/* SPDX-License-Identifier: GPL-2.0-or-later */
/* Only flashrom's SPI25-compatible catalogue and serprog transport are exposed. */
#include "flash.h"

struct frwr_chip_info {
    const char *vendor;
    const char *name;
    size_t capacity;
    unsigned int erased_value;
    unsigned int capabilities; /* read=1, write=2, erase=4; excludes BAD/NA */
};

static void describe(const struct flashchip *chip, struct frwr_chip_info *out)
{
    out->vendor = chip->vendor;
    out->name = chip->name;
    out->capacity = (size_t)chip->total_size * 1024;
    out->erased_value = (chip->feature_bits & FEATURE_ERASED_ZERO) ? 0 : 255;
    out->capabilities = 0;
    if (chip->read && chip->tested.read <= NT) out->capabilities |= 1;
    if (chip->write && chip->tested.write <= NT) out->capabilities |= 2;
    if (chip->tested.erase <= NT) {
        for (unsigned int i = 0; i < NUM_ERASEFUNCTIONS; i++)
            if (chip->block_erasers[i].block_erase) out->capabilities |= 4;
    }
}

int frwr_catalog_get(size_t index, struct frwr_chip_info *out)
{
    size_t n = 0;
    for (unsigned int i = 0; i < flashchips_size; i++) {
        const struct flashchip *chip = &flashchips[i];
        if (!chip->name || chip->bustype != BUS_SPI || chip->spi_cmd_set != SPI25) continue;
        if (n++ == index) { describe(chip, out); return 1; }
    }
    return 0;
}

int frwr_context_info(struct flashctx *ctx, struct frwr_chip_info *out)
{
    if (!ctx || !ctx->chip) return 0;
    describe(ctx->chip, out);
    return 1;
}

/* Cancellation is observed between bounded SPI reads; never during mutation. */
int frwr_read_region(struct flashctx *ctx, unsigned char *data, size_t start, size_t len,
                     int (*cancelled)(void *), void *user)
{
    if (start > UINT32_MAX || len > UINT32_MAX || start + len > flashrom_flash_getsize(ctx)) return 1;
    if (prepare_flash_access(ctx, true, false, false, false)) return 1;
    ctx->stage_progress[FLASHROM_PROGRESS_READ].current = 0;
    ctx->stage_progress[FLASHROM_PROGRESS_READ].total = len;
    int result = 0;
    for (size_t done = 0; done < len;) {
        if (cancelled && cancelled(user)) { result = -2; break; }
        size_t count = len - done;
        if (count > 4096) count = 4096;
        result = read_flash(ctx, data + done, (unsigned int)(start + done), (unsigned int)count);
        if (result) break;
        done += count;
    }
    finalize_flash_access(ctx);
    return result;
}
