#include <assert.h>
#include <stdio.h>

#define main firmware_main
#include "../main.c"
#undef main

static uint8_t host_request[8192], host_response[8192];
static size_t request_offset, request_size, response_size;
static uint32_t mock_time_ms;
static bool chip_selected, mounted = true;
static bool pin_level[32];
static uint8_t flash_reply = 0xef;
static unsigned transfers;

uint32_t get_absolute_time(void) { return mock_time_ms; }

void gpio_put(uint pin, bool value)
{
    pin_level[pin] = value;
    if (pin == PIN_CS) {
        chip_selected = !value;
    }
}

void gpio_set_dir(uint pin, bool value) {}
void gpio_set_function(uint pin, int function) {}
void gpio_init(uint pin) {}
void gpio_disable_pulls(uint pin) {}
void gpio_pull_up(uint pin) {}
uint32_t spi_set_baudrate(int port, uint32_t hz) { return hz; }
void spi_set_format(int port, int bits, int cpol, int cpha, int order) {}

int spi_write_blocking(int port, const uint8_t *data, size_t size)
{
    assert(chip_selected && spi_enabled);
    ++transfers;
    return (int)size;
}

int spi_read_blocking(int port, uint8_t dummy, uint8_t *data, size_t size)
{
    assert(chip_selected && spi_enabled);
    memset(data, flash_reply, size);
    return (int)size;
}

void tusb_init(int port, const tusb_rhport_init_t *config) {}
void tud_task(void) {}
bool tud_ready(void) { return mounted; }
bool tud_mounted(void) { return mounted; }

unsigned tud_cdc_read(void *data, unsigned size)
{
    size = MIN(MIN(size, request_size - request_offset), 7);
    memcpy(data, host_request + request_offset, size);
    request_offset += size;
    return size;
}

unsigned tud_cdc_write(const void *data, unsigned size)
{
    assert(response_size + size <= sizeof host_response);
    memcpy(host_response + response_size, data, size);
    response_size += size;
    return size;
}

unsigned tud_cdc_write_available(void) { return 11; }
void tud_cdc_write_flush(void) {}
void tud_cdc_read_flush(void) { request_offset = request_size; }
void tud_cdc_write_clear(void) { response_size = 0; }

static void send(const uint8_t *data, size_t size)
{
    assert(size <= sizeof host_request);
    memcpy(host_request, data, size);
    request_offset = response_size = 0;
    request_size = size;

    for (unsigned i = 0; i < 4000 && (request_offset < request_size || reply_size); ++i) {
        serprog_task();
    }
    assert(request_offset == request_size && reply_size == 0);
}

#define SEND(...) send((const uint8_t[]){__VA_ARGS__}, sizeof((const uint8_t[]){__VA_ARGS__}))

static void test_serprog(void)
{
    SEND(0x10, 0x01, 0x02);
    assert(response_size == 38);
    assert(memcmp(host_response, "\x15\x06\x06\x01\x00\x06\x3f\x01\x7f", 9) == 0);
    for (size_t i = 9; i < response_size; ++i) {
        assert(host_response[i] == 0);
    }

    SEND(0x13, 1, 0, 0, 3, 0, 0, 0x9f);
    assert(response_size == 1 && host_response[0] == 0x15 && transfers == 0);

    SEND(0x05, 0x12, 0x08, 0x15, 1, 0x16, 0, 0x16, 1, 0x14, 0, 0, 0, 0);
    assert(response_size == 7);
    assert(memcmp(host_response, "\x06\x08\x06\x06\x06\x15\x15", 7) == 0);

    // Read/write limits and setting the clock to 1 MHz.
    SEND(0x08, 0x11, 0x14, 0x40, 0x42, 0x0f, 0x00);
    assert(response_size == 13);
    assert(memcmp(host_response, "\x06\x00\x10\x00\x06\x00\x10\x00\x06\x40\x42\x0f\x00", 13) == 0);

    SEND(0x13, 1, 0, 0, 0, 16, 0, 0x9f);
    assert(response_size == 4097 && host_response[0] == 0x06);
    assert(host_response[4096] == 0xef && transfers == 1 && !chip_selected);

    SEND(0x13, 2, 0, 0, 1, 16, 0, 0x15, 0, 0x10);
    assert(response_size == 3 && memcmp(host_response, "\x15\x15\x06", 3) == 0);
    assert(spi_enabled);

    SEND(0x13, 2, 0);
    assert(response_size == 0);
    mock_time_ms += FRAME_TIMEOUT_MS;
    SEND(0x10);
    assert(response_size == 3 && memcmp(host_response, "\x15\x15\x06", 3) == 0);
}

static void expect_leds(bool write, bool read)
{
    leds_task();
    assert(pin_level[LED_WR] == write && pin_level[LED_RD] == read);
    assert(pin_level[PICO_DEFAULT_LED_PIN] == mounted);
}

static void test_leds_and_disconnect(void)
{
    mock_time_ms += 1;
    SEND(0x13, 4, 0, 0, 0, 0, 0, 0x20, 0, 0, 0); // Sector erase
    expect_leds(true, false);

    mock_time_ms += 250;
    expect_leds(true, false);
    flash_reply = 1;
    SEND(0x13, 1, 0, 0, 1, 0, 0, 0x05);
    assert(response_size == 2 && host_response[1] == 1);
    expect_leds(true, false);

    mock_time_ms += 250;
    flash_reply = 0;
    SEND(0x13, 1, 0, 0, 1, 0, 0, 0x05);
    expect_leds(true, false);
    mock_time_ms += 120;
    expect_leds(false, false);

    SEND(0x13, 1, 0, 0, 1, 0, 0, 0x05); // Standalone status read
    expect_leds(false, true);
    mock_time_ms += 1000;
    expect_leds(true, true);
    mock_time_ms += 100;
    expect_leds(false, false);
    mock_time_ms += 900;
    expect_leds(true, true);

    SEND(0x13, 4, 0, 0, 0, 0, 0, 0x20, 0, 0, 0);
    tud_cdc_line_state_cb(0, true, false);
    tud_cdc_line_state_cb(0, false, false);
    assert(!spi_enabled && !chip_selected);
    mock_time_ms += 250;
    expect_leds(false, false);

    mounted = false;
    tud_umount_cb();
    expect_leds(false, false);
    mounted = true;
    tud_mount_cb();
    SEND(0x10);
    assert(response_size == 2 && memcmp(host_response, "\x15\x06", 2) == 0);
    expect_leds(false, false);
}

int main(void)
{
    test_serprog();
    test_leds_and_disconnect();
    puts("serprog and LED tests passed (simulated CDC, SPI and GPIO)");
    return 0;
}
