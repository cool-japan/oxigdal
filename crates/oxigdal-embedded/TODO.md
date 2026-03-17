# TODO: oxigdal-embedded

## High Priority
- [ ] Verify compilation on actual embedded targets (thumbv7em, riscv32imc, xtensa)
- [ ] Implement fixed-point arithmetic for coordinate transforms (no FPU targets)
- [ ] Add DMA-aware buffer management for zero-copy peripheral transfers
- [ ] Implement real-time deadline-aware processing with priority scheduling
- [ ] Add stack usage analysis annotations for all public functions

## Medium Priority
- [ ] Implement SPI/I2C driver abstraction for GPS module integration
- [ ] Add NMEA sentence parser in no_std for embedded GPS receivers
- [ ] Implement lightweight tile renderer for embedded displays (SPI LCD, e-ink)
- [ ] Add flash storage driver for tile cache persistence on NOR/NAND flash
- [ ] Implement watchdog integration to prevent processing hangs
- [ ] Add power mode transitions (active/sleep/deep-sleep) with state preservation
- [ ] Implement compile-time memory layout verification for StaticPool
- [ ] Add defmt logging integration for efficient embedded debugging

## Low Priority / Future
- [ ] Implement LoRa/LoRaWAN packet encoder for geospatial telemetry
- [ ] Add BLE beacon support for indoor positioning augmentation
- [ ] Implement sensor fusion (accelerometer + GPS) for dead reckoning
- [ ] Add embedded-hal trait implementations for platform abstraction
- [ ] Implement OTA firmware update support with rollback
- [ ] Add RTOS integration (FreeRTOS, Zephyr) task wrappers
- [ ] Implement minimal vector renderer for embedded map display
- [ ] Add hardware CRC acceleration for data integrity on constrained links
