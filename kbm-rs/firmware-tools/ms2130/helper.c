/* SPDX-License-Identifier: GPL-2.0-or-later */
/* Separate process using the existing frwr libflashrom/serprog engine.
 * The original/target/readback remain in RAM. Only the selected input ROM is a file.
 * No mutation occurs until the parent explicitly sends Y after ready_to_write. */
#include <windows.h>
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <stdbool.h>
#include <string.h>
#include <wchar.h>
#include <io.h>
#include <fcntl.h>
#include "libflashrom.h"

struct frwr_chip_info { const char *vendor, *name; size_t capacity; unsigned int erased_value, capabilities; };
int frwr_catalog_get(size_t, struct frwr_chip_info *);
int frwr_context_info(void *, struct frwr_chip_info *);
int frwr_read_region(void *, unsigned char *, size_t, size_t, int (*)(void *), void *);
#define MAX_IMAGE (64u * 1024u * 1024u)

static void quoted(const char *s) {
    putchar('"');
    for (; s && *s; s++) {
        unsigned char c = (unsigned char)*s;
        if (c=='"' || c=='\\') putchar('\\');
        if (c>=32) putchar(c); else printf("\\u%04x",c);
    }
    putchar('"');
}
static void event(const char *name) { printf("{\"event\":"); quoted(name); puts("}"); fflush(stdout); }
static void failure(const char *code) { printf("{\"event\":\"error\",\"code\":"); quoted(code); puts("}"); fflush(stdout); }
static void definition(struct frwr_chip_info *info) {
    printf("{\"vendor\":"); quoted(info->vendor); printf(",\"name\":"); quoted(info->name);
    printf(",\"capacity\":%llu,\"capabilities\":%u}",(unsigned long long)info->capacity,info->capabilities);
}
static void progress(enum flashrom_progress_stage stage, size_t current, size_t total, void *user) {
    (void)user;
    if (current!=total && current%4096!=0) return;
    printf("{\"event\":\"progress\",\"stage\":%d,\"done\":%llu,\"total\":%llu}\n",
        (int)stage,(unsigned long long)current,(unsigned long long)total); fflush(stdout);
}
static void log_message(enum flashrom_log_level level, const char *message, void *user) {
    (void)user;
    if (level<=FLASHROM_MSG_INFO && message) fputs(message,stderr);
}
static char *utf8(const wchar_t *text) {
    int len=WideCharToMultiByte(CP_UTF8,WC_ERR_INVALID_CHARS,text,-1,NULL,0,NULL,NULL);
    if (len<=0) return NULL;
    char *result=malloc((size_t)len);
    if (result && !WideCharToMultiByte(CP_UTF8,WC_ERR_INVALID_CHARS,text,-1,result,len,NULL,NULL)) { free(result); return NULL; }
    return result;
}
static bool read_exact(HANDLE port, unsigned char *data, DWORD len) {
    while (len) { DWORD done=0; if (!ReadFile(port,data,len,&done,NULL) || !done) return false; data+=done; len-=done; }
    return true;
}
static bool serial_command(HANDLE port, unsigned char opcode, unsigned char *data, DWORD len) {
    DWORD done=0;
    if (!WriteFile(port,&opcode,1,&done,NULL) || done!=1) return false;
    return read_exact(port,data,len);
}
static bool is_frwr(const wchar_t *name) {
    if (wcsncmp(name,L"COM",3) || !name[3] || wcslen(name)>12) return false;
    for (const wchar_t *p=name+3;*p;p++) if (*p<L'0'||*p>L'9') return false;
    wchar_t path[32]; _snwprintf(path,32,L"\\\\.\\%ls",name);
    HANDLE port=CreateFileW(path,GENERIC_READ|GENERIC_WRITE,0,NULL,OPEN_EXISTING,0,NULL);
    if (port==INVALID_HANDLE_VALUE) return false;
    DCB dcb={0}; dcb.DCBlength=sizeof(dcb);
    bool ok=GetCommState(port,&dcb)!=0;
    dcb.BaudRate=115200; dcb.ByteSize=8; dcb.Parity=NOPARITY; dcb.StopBits=ONESTOPBIT;
    dcb.fBinary=TRUE; dcb.fParity=FALSE; dcb.fOutxCtsFlow=FALSE; dcb.fOutxDsrFlow=FALSE;
    dcb.fDtrControl=DTR_CONTROL_ENABLE; dcb.fRtsControl=RTS_CONTROL_DISABLE;
    dcb.fOutX=FALSE; dcb.fInX=FALSE;
    dcb.fDsrSensitivity=FALSE;
    COMMTIMEOUTS timeouts={MAXDWORD,0,3000,0,3000};
    ok=ok && SetCommState(port,&dcb) && SetCommTimeouts(port,&timeouts);
    Sleep(100); PurgeComm(port,PURGE_RXCLEAR|PURGE_TXCLEAR);
    unsigned char b[17];
    ok=ok && serial_command(port,0x10,b,2) && b[0]==0x15 && b[1]==0x06;
    ok=ok && serial_command(port,0x01,b,3) && b[0]==6 && b[1]==1 && b[2]==0;
    ok=ok && serial_command(port,0x03,b,17) && b[0]==6 && !memcmp(b+1,"frwr\0",5);
    EscapeCommFunction(port,CLRDTR); CloseHandle(port); return ok;
}

int wmain(int argc, wchar_t **argv) {
    _setmode(_fileno(stdout),_O_BINARY);
    if (argc==2 && !wcscmp(argv[1],L"--catalog")) {
        struct frwr_chip_info info;
        printf("{\"event\":\"catalog\",\"chips\":[");
        for (size_t n=0;n<10000 && frwr_catalog_get(n,&info);n++) { if(n) putchar(','); definition(&info); }
        puts("]}"); return 0;
    }
    bool write=argc==5 && !wcscmp(argv[1],L"--write");
    bool probe=argc==3 && !wcscmp(argv[1],L"--probe");
    if (!write && !probe) { failure("INVALID_ARGUMENTS"); return 2; }
    if (!is_frwr(argv[2])) { failure("FRWR_IDENTITY_FAILED"); return 3; }
    char *port=utf8(argv[2]); const char *model=NULL;
    if (!port) { failure("INVALID_TEXT"); return 2; }
    struct flashrom_flashctx *ctx=NULL;
    struct flashrom_programmer *programmer=NULL;
    bool started=false; int result=4; unsigned char *target=NULL,*readback=NULL;
    struct frwr_chip_info info={0};
    flashrom_set_log_callback_v2(log_message,NULL); flashrom_set_log_level(FLASHROM_MSG_INFO);
    if (flashrom_init(1) || flashrom_create_context(&ctx)) { failure("ENGINE_INIT_FAILED"); goto cleanup; }
    char params[128]; snprintf(params,sizeof(params),"dev=%s:115200,spispeed=1000000",port);
    started=true;
    if (flashrom_programmer_init(&programmer,"serprog",params)) { failure("SERPROG_OPEN_FAILED"); goto cleanup; }
    flashrom_set_progress_callback_v2(ctx,progress,NULL);
    const char **candidates=NULL;
    int count=flashrom_flash_probe_v2(ctx,&candidates,programmer,model);
    /* The board uses the user-confirmed BV/DV variant. These two RDID aliases
     * otherwise make flashrom request a manual model selection. Resolve this
     * known board profile internally; no model choice is required in the app.
     * Do not choose an arbitrary first definition for other ambiguous chips. */
    if (!model && count==2 && candidates &&
        ((!strcmp(candidates[0],"W25Q80BV/W25Q80DV") && !strcmp(candidates[1],"W25Q80RV")) ||
         (!strcmp(candidates[1],"W25Q80BV/W25Q80DV") && !strcmp(candidates[0],"W25Q80RV")))) {
        flashrom_data_free((void*)candidates); candidates=NULL;
        count=flashrom_flash_probe_v2(ctx,&candidates,programmer,"W25Q80BV/W25Q80DV");
    }
    printf("{\"event\":\"identified\",\"candidates\":[");
    for (int i=0;i<count;i++) { if(i) putchar(','); quoted(candidates[i]); }
    printf("],\"chip\":");
    bool ready=count==1 && frwr_context_info(ctx,&info) && info.capacity>0 && info.capacity<=MAX_IMAGE;
    if (ready) definition(&info); else printf("null");
    puts("}"); fflush(stdout);
    if (candidates) flashrom_data_free((void*)candidates);
    if (!write) { result=count>=0?0:4; goto cleanup; }
    wchar_t *end=NULL; unsigned long long expected=wcstoull(argv[3],&end,10);
    if (!ready || !end || *end || expected!=info.capacity || (info.capabilities&7)!=7) { failure("CHIP_CHANGED_OR_UNSUPPORTED"); goto cleanup; }
    FILE *source=_wfopen(argv[4],L"rb");
    if (!source) { failure("IMAGE_OPEN_FAILED"); goto cleanup; }
    if (_fseeki64(source,0,SEEK_END)) { fclose(source); failure("IMAGE_IO_FAILED"); goto cleanup; }
    long long length=_ftelli64(source);
    if (length<=0 || (unsigned long long)length>info.capacity || _fseeki64(source,0,SEEK_SET)) {
        fclose(source); failure("IMAGE_TOO_LARGE"); goto cleanup;
    }
    target=malloc(info.capacity); readback=malloc(info.capacity);
    if (!target || !readback) { fclose(source); failure("OUT_OF_MEMORY"); goto cleanup; }
    event("reading_original");
    if (frwr_read_region(ctx,target,0,info.capacity,NULL,NULL)) { fclose(source); failure("ORIGINAL_READ_FAILED"); goto cleanup; }
    bool source_ok=fread(target,1,(size_t)length,source)==(size_t)length;
    if (fclose(source)) source_ok=false;
    if (!source_ok) { failure("IMAGE_IO_FAILED"); goto cleanup; }
    event("ready_to_write");
    if (getchar()!='Y') { event("cancelled"); result=0; goto cleanup; }
    event("commit_started");
    flashrom_flag_set(ctx,FLASHROM_FLAG_VERIFY_AFTER_WRITE,true);
    flashrom_flag_set(ctx,FLASHROM_FLAG_VERIFY_WHOLE_CHIP,true);
    if (flashrom_image_write(ctx,target,info.capacity,NULL)) { failure("WRITE_OR_VERIFY_FAILED"); goto cleanup; }
    event("reading_verification");
    if (frwr_read_region(ctx,readback,0,info.capacity,NULL,NULL) || memcmp(target,readback,info.capacity)) { failure("FULL_READBACK_MISMATCH"); goto cleanup; }
    printf("{\"event\":\"completed\",\"verified\":true,\"tail_preserved\":true,\"image_bytes\":%llu,\"verified_bytes\":%llu}\n",
        (unsigned long long)length,(unsigned long long)info.capacity); fflush(stdout); result=0;
cleanup:
    free(target); free(readback);
    if(ctx) flashrom_flash_release(ctx);
    if(started) flashrom_programmer_shutdown(programmer);
    flashrom_shutdown(); flashrom_set_log_callback_v2(NULL,NULL);
    free(port); return result;
}
