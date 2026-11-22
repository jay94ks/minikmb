#include "native_dhcp.h"
#include <string.h>

static const uint8_t server_ip[4] = {192,168,127,1};
static uint16_t be16(const uint8_t *p) { return (uint16_t)((p[0]<<8)|p[1]); }
static void put16(uint8_t *p, uint16_t n) { p[0]=(uint8_t)(n>>8); p[1]=(uint8_t)n; }
static uint16_t checksum(const uint8_t *p, uint16_t n) {
    uint32_t sum=0;
    while(n>1) { sum+=be16(p); p+=2; n-=2; }
    if(n) sum+=(uint16_t)*p<<8;
    while(sum>>16) sum=(sum&65535)+(sum>>16);
    return (uint16_t)~sum;
}
void kbm_dhcp_init(kbm_dhcp_t *s, const uint8_t mac[6]) { memset(s,0,sizeof(*s)); memcpy(s->mac,mac,6); }
void kbm_dhcp_forget(kbm_dhcp_t *s, uint8_t ingress) {
    for(unsigned i=0;i<8;i++) if(s->leases[i].ingress==ingress) s->leases[i].used=false;
}
static bool ip_equal(const uint8_t *p, uint8_t host) {
    return p[0]==192 && p[1]==168 && p[2]==127 && p[3]==host;
}
static kbm_dhcp_lease_t *lease(kbm_dhcp_t *s, uint8_t ingress, const uint8_t *mac, uint32_t now) {
    kbm_dhcp_lease_t *free_slot=NULL;
    for(unsigned i=0;i<8;i++) {
        kbm_dhcp_lease_t *l=&s->leases[i];
        if(l->used && l->ingress!=2 && (int32_t)(now-l->deadline)>=0) l->used=false;
        if(l->used && l->ingress==ingress && memcmp(l->mac,mac,6)==0) return l;
        if(!l->used) free_slot=l;
    }
    if(!free_slot) return NULL;
    uint8_t host=ingress==2 ? 1 : (uint8_t)(100+ingress);
    for(unsigned attempt=0;attempt<100;attempt++) {
        bool busy=false;
        for(unsigned i=0;i<8;i++) if(s->leases[i].used && s->leases[i].address==host) busy=true;
        if(!busy) {
            *free_slot=(kbm_dhcp_lease_t){.used=true,.ingress=ingress,.address=host,.deadline=now+60000u};
            memcpy(free_slot->mac,mac,6); return free_slot;
        }
        if(ingress==2) return NULL; // Never assign another address to the master PC.
        host=host<102 ? 102 : (uint8_t)(host+1);
        if(host>199) host=102;
    }
    return NULL;
}
bool kbm_dhcp_process(kbm_dhcp_t *s, uint8_t ingress, uint32_t now,
                      const kbm_net_frame_t *in, kbm_net_frame_t *out) {
    out->length=0;
    const uint8_t *f=in->data;
    if(in->length<42 || in->length>KBM_NET_MTU || be16(f+12)!=0x0800 || f[14]>>4!=4 || f[23]!=17) return false;
    const unsigned ihl=(f[14]&15u)*4u;
    if(ihl<20 || 14+ihl+8>in->length) return false;
    const uint8_t *udp=f+14+ihl;
    if(be16(udp+2)!=67) return false;
    // Consume client DHCP locally; .1 ARP and other traffic still belong to the PC.
    const unsigned ip_len=be16(f+16), udp_len=be16(udp+4);
    if(ingress>2 || be16(udp)!=68 || (be16(f+20)&0x3fffu) ||
       ip_len<ihl+8 || 14+ip_len>in->length || udp_len<248 || udp_len>ip_len-ihl ||
       checksum(f+14,(uint16_t)ihl)!=0) return true;
    if(be16(udp+6)) {
        uint32_t sum=0;
        for(unsigned i=26;i<34;i+=2) sum+=be16(f+i);
        sum+=17+udp_len;
        for(unsigned i=0;i+1<udp_len;i+=2) sum+=be16(udp+i);
        if(udp_len&1) sum+=(uint16_t)udp[udp_len-1]<<8;
        while(sum>>16) sum=(sum&65535)+(sum>>16);
        if(sum!=65535) return true;
    }
    const uint8_t *b=udp+8;
    if(b[0]!=1 || b[1]!=1 || b[2]!=6 || (b[28]&1) || memcmp(b+24,"\0\0\0\0",4) ||
       memcmp(b+236,"\x63\x82\x53\x63",4)) return true;
    uint8_t type=0, requested[4]={0}, sid[4]={0};
    bool has_request=false, has_sid=false, ended=false;
    const uint8_t *client_id=NULL;
    uint8_t client_id_len=0;
    for(unsigned at=240;at<udp_len-8;) {
        uint8_t code=b[at++];
        if(code==255) { ended=true; break; }
        if(code==0) continue;
        if(at>=udp_len-8) return true;
        unsigned n=b[at++];
        if(at+n>udp_len-8) return true;
        if(code==53) { if(n!=1 || type) return true; type=b[at]; }
        if(code==50) { if(n!=4 || has_request) return true; memcpy(requested,b+at,4); has_request=true; }
        if(code==54) { if(n!=4 || has_sid) return true; memcpy(sid,b+at,4); has_sid=true; }
        if(code==52) return true; // No overloaded options: fail closed.
        if(code==61) { if(n<2 || client_id) return true; client_id=b+at; client_id_len=(uint8_t)n; }
        at+=n;
    }
    if(!ended || (has_sid && memcmp(sid,server_ip,4))) return true;
    if(type!=1 && type!=3 && type!=4 && type!=7 && type!=8) return true;
    if(type==4 || type==7) {
        for(unsigned i=0;i<8;i++) if(s->leases[i].used && s->leases[i].ingress==ingress &&
            !memcmp(s->leases[i].mac,b+28,6)) s->leases[i].used=false;
        return true;
    }
    kbm_dhcp_lease_t *l=type==8 ? NULL : lease(s,ingress,b+28,now);
    if(type!=8 && !l) return true;
    uint8_t response=type==1 ? 2 : 5;
    if(type==3 && ((has_request && !ip_equal(requested,l->address)) ||
        (!has_request && !ip_equal(b+12,l->address)))) response=6;
    if(l && response==5) l->deadline=now+3600000u;
    uint8_t *r=out->data;
    memset(r,0,14+20+8+300);
    // Broadcast offers/initial ACKs/NAKs, unicast renewal/INFORM to ciaddr.
    bool unicast=response!=6 && !has_request && b[12]!=0 && !(be16(b+10)&0x8000u);
    if(unicast) memcpy(r,b+28,6); else memset(r,255,6);
    memcpy(r+6,s->mac,6); put16(r+12,0x0800);
    uint8_t *ip=r+14, *u=ip+20, *reply=u+8;
    ip[0]=0x45; ip[8]=64; ip[9]=17;
    memcpy(ip+12,server_ip,4);
    if(unicast) memcpy(ip+16,b+12,4); else memset(ip+16,255,4);
    put16(u,67); put16(u+2,68); // IPv4 UDP checksum zero is allowed.
    reply[0]=2; reply[1]=1; reply[2]=6;
    memcpy(reply+4,b+4,4); memcpy(reply+10,b+10,2); memcpy(reply+28,b+28,16);
    if(response!=6) {
        memcpy(reply+12,b+12,4);
        if(l) { memcpy(reply+16,server_ip,3); reply[19]=l->address; }
    }
    memcpy(reply+20,server_ip,4); memcpy(reply+236,b+236,4);
    unsigned n=240;
    reply[n++]=53; reply[n++]=1; reply[n++]=response;
    reply[n++]=54; reply[n++]=4; memcpy(reply+n,server_ip,4); n+=4;
    if(client_id) { reply[n++]=61; reply[n++]=client_id_len; memcpy(reply+n,client_id,client_id_len); n+=client_id_len; }
    if(response!=6) {
        reply[n++]=1; reply[n++]=4; memset(reply+n,255,3); reply[n+3]=0; n+=4;
        reply[n++]=3; reply[n++]=4; memcpy(reply+n,server_ip,4); n+=4;
        if(l) {
            reply[n++]=51; reply[n++]=4;
            if(ingress==2) memset(reply+n,255,4); // Infinite .1 lease avoids OS-local self-renewal.
            else { reply[n]=0; reply[n+1]=0; reply[n+2]=0x0e; reply[n+3]=0x10; }
            n+=4;
        }
    }
    reply[n++]=255;
    if(n<300) n=300;
    put16(u+4,(uint16_t)(8+n)); put16(ip+2,(uint16_t)(28+n)); put16(ip+10,checksum(ip,20));
    out->length=(uint16_t)(42+n);
    return true;
}
