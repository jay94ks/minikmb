#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
typedef unsigned uint;
#define PICO_DEFAULT_LED_PIN 25
#define GPIO_IN 0
#define GPIO_OUT 1
#define GPIO_FUNC_SPI 1
#define GPIO_FUNC_SIO 0
#define MIN(a,b) ((a)<(b)?(a):(b))
#define MAX(a,b) ((a)>(b)?(a):(b))
#define count_of(a) (sizeof(a)/sizeof((a)[0]))
uint32_t get_absolute_time(void);
#define to_ms_since_boot(x) (x)
void gpio_put(uint,bool);
void gpio_set_dir(uint,bool);
void gpio_set_function(uint,int);
void gpio_init(uint);
void gpio_disable_pulls(uint);
void gpio_pull_up(uint);
