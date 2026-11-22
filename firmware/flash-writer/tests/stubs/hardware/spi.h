#define spi0 0
#define SPI_CPOL_0 0
#define SPI_CPHA_0 0
#define SPI_MSB_FIRST 0
uint32_t spi_set_baudrate(int,uint32_t);
#define spi_init spi_set_baudrate
void spi_set_format(int,int,int,int,int);
int spi_write_blocking(int,const uint8_t *,size_t);
int spi_read_blocking(int,uint8_t,uint8_t *,size_t);
