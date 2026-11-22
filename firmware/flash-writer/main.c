#include <stdbool.h>
#include <stdint.h>
#include <string.h>

#include "hardware/spi.h"
#include "pico/stdlib.h"
#include "tusb.h"

enum {
    PIN_SCK = 2, PIN_TX = 3, PIN_RX = 4, PIN_CS = 5,
    PIN_WP = 6, PIN_HOLD = 7, LED_WR = 14, LED_RD = 15,
    SPI_LIMIT = 4096, MIN_HZ = 100000, DEFAULT_HZ = 1000000, MAX_HZ = 12000000,
    FRAME_TIMEOUT_MS = 2000, LED_HOLD_MS = 120,
    IDLE_AFTER_MS = 1000, IDLE_PERIOD_MS = 1000, IDLE_PULSE_MS = 100,
    ACK = 0x06, NAK = 0x15, BUS_SPI = 0x08,
    NOP = 0x00, Q_IFACE = 0x01, Q_CMDMAP = 0x02, Q_PGMNAME = 0x03,
    Q_SERBUF = 0x04, Q_BUSTYPE = 0x05, Q_WRNMAXLEN = 0x08,
    SYNCNOP = 0x10, Q_RDNMAXLEN = 0x11, S_BUSTYPE = 0x12,
    O_SPIOP = 0x13, S_SPI_FREQ = 0x14, S_PIN_STATE = 0x15, S_SPI_CS = 0x16,
    W25Q_READ_STATUS1 = 0x05, W25Q_STATUS_BUSY = 0x01,
};

static const uint8_t command_map[32] = {
    [0] = (1u << NOP) | (1u << Q_IFACE) | (1u << Q_CMDMAP) |
          (1u << Q_PGMNAME) | (1u << Q_SERBUF) | (1u << Q_BUSTYPE),
    [1] = 1u << (Q_WRNMAXLEN - 8),
    [2] = (1u << (SYNCNOP - 16)) | (1u << (Q_RDNMAXLEN - 16)) |
          (1u << (S_BUSTYPE - 16)) | (1u << (O_SPIOP - 16)) |
          (1u << (S_SPI_FREQ - 16)) | (1u << (S_PIN_STATE - 16)) |
          (1u << (S_SPI_CS - 16)),
};

static enum { COMMAND, ARGUMENTS, SPI_PAYLOAD, DISCARD_PAYLOAD } phase;
static uint8_t command, args[6], spi_tx[SPI_LIMIT], reply[SPI_LIMIT + 1];
static uint32_t received, expected, read_length, frame_progress_ms;
static uint32_t reply_size, reply_sent, last_operation_ms;
static uint32_t last_read_ms, last_write_ms;
static bool spi_enabled, had_dtr, write_pending;

static uint32_t now_ms(void)
{
    return to_ms_since_boot(get_absolute_time());
}

static void set_spi_enabled(bool enable)
{
    if (enable) {
        gpio_put(PIN_CS, true);
        gpio_set_dir(PIN_CS, GPIO_OUT);
        gpio_set_function(PIN_SCK, GPIO_FUNC_SPI);
        gpio_set_function(PIN_TX, GPIO_FUNC_SPI);
        gpio_set_function(PIN_RX, GPIO_FUNC_SPI);
    } else {
        write_pending = false;
        gpio_set_dir(PIN_CS, GPIO_IN);
        for (uint pin = PIN_SCK; pin <= PIN_RX; ++pin) {
            gpio_set_dir(pin, GPIO_IN);
            gpio_set_function(pin, GPIO_FUNC_SIO);
        }
    }
    spi_enabled = enable;
}

static void prepare_reply(uint8_t status, uint32_t length)
{
    reply[0] = status;
    reply_size = length + 1;
    reply_sent = 0;
}

static void reply_number(uint32_t value, uint32_t length)
{
    for (uint32_t i = 0; i < length; ++i) {
        reply[i + 1] = (uint8_t)(value >> (8 * i));
    }
    prepare_reply(ACK, length);
}

static uint32_t load_le(const uint8_t *data, uint32_t length)
{
    uint32_t value = 0;
    for (uint32_t i = 0; i < length; ++i) {
        value |= (uint32_t)data[i] << (8 * i);
    }
    return value;
}

static bool starts_w25q_write(uint8_t opcode)
{
    switch (opcode) {
    case 0x01: case 0x31: case 0x11:       // Write status registers
    case 0x02: case 0x12:                  // Page program, 3-byte or 4-byte address
    case 0x20: case 0x21:                  // Sector erase
    case 0x52: case 0xd8: case 0xdc:       // Block erase
    case 0x60: case 0xc7:                  // Chip erase
        return true;
    default:
        return false;
    }
}

static void spi_operation(void)
{
    phase = COMMAND;
    if (!spi_enabled) {
        prepare_reply(NAK, 0);
        return;
    }

    const uint8_t opcode = expected != 0 ? spi_tx[0] : 0;
    // A BUSY poll belongs to the write or erase that it is checking.
    const bool polling_write = write_pending && expected != 0 &&
                               opcode == W25Q_READ_STATUS1 && read_length != 0;
    const bool write_activity = read_length == 0 || polling_write;
    gpio_put(write_activity ? LED_WR : LED_RD, true);
    gpio_put(PIN_CS, false);
    if (expected != 0) {
        spi_write_blocking(spi0, spi_tx, expected);
    }
    if (read_length != 0) {
        spi_read_blocking(spi0, 0xff, reply + 1, read_length);
    }
    gpio_put(PIN_CS, true);
    if (polling_write) {
        write_pending = (reply[read_length] & W25Q_STATUS_BUSY) != 0;
    } else {
        write_pending = write_activity && starts_w25q_write(opcode);
    }
    last_operation_ms = now_ms();
    if (write_activity) {
        last_write_ms = last_operation_ms;
    } else {
        last_read_ms = last_operation_ms;
    }
    prepare_reply(ACK, read_length);
}

static void start_command(void)
{
    static const uint8_t name[16] = "frwr";
    received = 0;

    switch (command) {
    case NOP:
        prepare_reply(ACK, 0);
        return;
    case Q_IFACE:
        reply_number(1, 2);
        return;
    case Q_CMDMAP:
        memcpy(reply + 1, command_map, sizeof command_map);
        prepare_reply(ACK, sizeof command_map);
        return;
    case Q_PGMNAME:
        memcpy(reply + 1, name, sizeof name);
        prepare_reply(ACK, sizeof name);
        return;
    case Q_SERBUF:
        reply_number(CFG_TUD_CDC_RX_BUFSIZE, 2);
        return;
    case Q_BUSTYPE:
        reply_number(BUS_SPI, 1);
        return;
    case Q_WRNMAXLEN:
    case Q_RDNMAXLEN:
        reply_number(SPI_LIMIT, 3);
        return;
    case SYNCNOP:
        reply[1] = ACK;
        prepare_reply(NAK, 1);
        return;
    case S_BUSTYPE:
    case S_PIN_STATE:
    case S_SPI_CS:
        expected = 1;
        break;
    case S_SPI_FREQ:
        expected = 4;
        break;
    case O_SPIOP:
        expected = 6;
        break;
    default:
        prepare_reply(NAK, 0);
        return;
    }
    phase = ARGUMENTS;
}

static void finish_arguments(void)
{
    phase = COMMAND;
    switch (command) {
    case S_BUSTYPE:
        prepare_reply((args[0] & BUS_SPI) ? ACK : NAK, 0);
        break;
    case S_PIN_STATE:
        if (args[0] > 1) {
            prepare_reply(NAK, 0);
        } else {
            set_spi_enabled(args[0] != 0);
            prepare_reply(ACK, 0);
        }
        break;
    case S_SPI_CS:
        prepare_reply(args[0] == 0 ? ACK : NAK, 0);
        break;
    case S_SPI_FREQ: {
        uint32_t hz = load_le(args, 4);
        if (hz == 0) {
            prepare_reply(NAK, 0);
        } else {
            hz = MIN(MAX(hz, MIN_HZ), MAX_HZ);
            reply_number(spi_set_baudrate(spi0, hz), 4);
        }
        break;
    }
    case O_SPIOP:
        expected = load_le(args, 3);
        read_length = load_le(args + 3, 3);
        received = 0;
        if (expected > SPI_LIMIT || read_length > SPI_LIMIT) {
            if (expected == 0) {
                prepare_reply(NAK, 0);
            } else {
                phase = DISCARD_PAYLOAD;
            }
        } else if (expected == 0 && read_length == 0) {
            prepare_reply(NAK, 0);
        } else if (expected == 0) {
            spi_operation();
        } else {
            phase = SPI_PAYLOAD;
        }
        break;
    }
}

static void serprog_task(void)
{
    uint32_t now = now_ms();
    if (reply_size != 0) {
        uint32_t count = MIN(reply_size - reply_sent, tud_cdc_write_available());
        if (count != 0) {
            reply_sent += tud_cdc_write(reply + reply_sent, count);
            tud_cdc_write_flush();
        }
        if (reply_sent == reply_size) {
            reply_size = 0;
        }
        return;
    }

    if (phase != COMMAND && now - frame_progress_ms >= FRAME_TIMEOUT_MS) {
        phase = COMMAND;
        prepare_reply(NAK, 0);
        return;
    }
    if (phase == COMMAND) {
        if (tud_cdc_read(&command, 1) == 1) {
            frame_progress_ms = now;
            start_command();
        }
        return;
    }

    uint8_t discard[64];
    uint8_t *dest = discard;
    uint32_t count = expected - received;
    if (phase == ARGUMENTS) {
        dest = args + received;
    } else if (phase == SPI_PAYLOAD) {
        dest = spi_tx + received;
    } else {
        count = MIN(count, sizeof discard);
    }
    count = tud_cdc_read(dest, count);
    if (count == 0) {
        return;
    }
    frame_progress_ms = now;
    received += count;
    if (received != expected) {
        return;
    }

    if (phase == ARGUMENTS) {
        finish_arguments();
    } else if (phase == SPI_PAYLOAD) {
        spi_operation();
    } else {
        phase = COMMAND;
        prepare_reply(NAK, 0);
    }
}

static void reset_session(void)
{
    phase = COMMAND;
    reply_size = 0;
    set_spi_enabled(false);
    spi_set_baudrate(spi0, DEFAULT_HZ);
}

void tud_mount_cb(void)
{
    reset_session();
    had_dtr = false;
}

void tud_umount_cb(void)
{
    reset_session();
    had_dtr = false;
}

void tud_cdc_line_state_cb(uint8_t interface, bool dtr, bool rts)
{
    (void)interface;
    (void)rts;
    if (had_dtr && !dtr) {
        reset_session();
        tud_cdc_read_flush();
        tud_cdc_write_clear();
    }
    had_dtr = dtr;
}

static void leds_task(void)
{
    const uint32_t now = now_ms();
    const uint32_t elapsed = now - last_operation_ms;
    const bool idle_pulse = elapsed >= IDLE_AFTER_MS &&
        (elapsed - IDLE_AFTER_MS) % IDLE_PERIOD_MS < IDLE_PULSE_MS;
    const bool write_in_progress = write_pending && elapsed < IDLE_AFTER_MS;
    gpio_put(PICO_DEFAULT_LED_PIN, tud_mounted());
    gpio_put(LED_WR, idle_pulse || write_in_progress || now - last_write_ms < LED_HOLD_MS);
    gpio_put(LED_RD, idle_pulse || now - last_read_ms < LED_HOLD_MS);
}

int main(void)
{
    const uint output_leds[] = { PICO_DEFAULT_LED_PIN, LED_WR, LED_RD };
    for (size_t i = 0; i < count_of(output_leds); ++i) {
        gpio_init(output_leds[i]);
        gpio_set_dir(output_leds[i], GPIO_OUT);
    }
    for (uint pin = PIN_SCK; pin <= PIN_HOLD; ++pin) {
        gpio_init(pin);
        gpio_disable_pulls(pin);
    }
    gpio_put(PIN_CS, true);
    gpio_pull_up(PIN_CS);
    gpio_pull_up(PIN_WP);
    gpio_pull_up(PIN_HOLD);
    spi_init(spi0, DEFAULT_HZ);
    spi_set_format(spi0, 8, SPI_CPOL_0, SPI_CPHA_0, SPI_MSB_FIRST);
    last_operation_ms = now_ms();
    last_read_ms = last_write_ms = last_operation_ms - LED_HOLD_MS;

    const tusb_rhport_init_t usb = { .role = TUSB_ROLE_DEVICE, .speed = TUSB_SPEED_FULL };
    tusb_init(0, &usb);
    while (true) {
        tud_task();
        if (tud_ready()) {
            serprog_task();
        }
        leds_task();
    }
}
