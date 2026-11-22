#define CFG_TUD_CDC_RX_BUFSIZE 256
#define TUSB_ROLE_DEVICE 0
#define TUSB_SPEED_FULL 0
typedef struct { int role; int speed; } tusb_rhport_init_t;
void tusb_init(int,const tusb_rhport_init_t *);
void tud_task(void);
bool tud_mounted(void);
bool tud_ready(void);
unsigned tud_cdc_read(void *,unsigned);
unsigned tud_cdc_write(const void *,unsigned);
unsigned tud_cdc_write_available(void);
void tud_cdc_write_flush(void);
void tud_cdc_read_flush(void);
void tud_cdc_write_clear(void);
