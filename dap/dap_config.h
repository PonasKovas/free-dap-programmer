#ifndef DAP_CONFIG_H_
#define DAP_CONFIG_H_

#include <stdbool.h>
#include <stdint.h>

#define DAP_CONFIG_PERFORMANCE_ATTR

// --- General Protocol Settings ---
#define DAP_CONFIG_ENABLE_SWD 1
#define DAP_CONFIG_ENABLE_JTAG 1
#define DAP_CONFIG_DEFAULT_PORT 1 // 1 = SWD, 2 = JTAG

#define DAP_CONFIG_PACKET_SIZE 64
#define DAP_CONFIG_PACKET_COUNT 1
#define DAP_CONFIG_JTAG_DEV_COUNT 8
#define DAP_CONFIG_DEFAULT_CLOCK 1000000U // 1 MHz default

#define DAP_CONFIG_DELAY_CONSTANT 8000U
#define DAP_CONFIG_FAST_CLOCK 2000000 // 2 MHz threshold

// --- STM32L4 GPIOA Register Map (Base: 0x48000000) ---
#define GPIOA_BASE 0x48000000UL
#define GPIOA_MODER (*(volatile uint32_t *)(GPIOA_BASE + 0x00))
#define GPIOA_OSPEEDR (*(volatile uint32_t *)(GPIOA_BASE + 0x08))
#define GPIOA_PUPDR (*(volatile uint32_t *)(GPIOA_BASE + 0x0C))
#define GPIOA_IDR (*(volatile uint32_t *)(GPIOA_BASE + 0x10))
#define GPIOA_BSRR (*(volatile uint32_t *)(GPIOA_BASE + 0x18))

// --- Pin Assignment Index Map ---
// PA3 = TMS   / SWDIO (Bidirectional SWD, Output JTAG)
// PA4 = TCK   / SWDCLK (Output)
// PA5 = TDO   / SWO    (Input)
// PA6 = TDI            (Output)
// PA7 = nRESET         (Output / Open Drain)

// --- Pin Write Macros (BSRR: set in lower 16 bits, reset in upper 16 bits) ---
#define DAP_CONFIG_SWDIO_TMS_write(val)                                        \
  (GPIOA_BSRR = (val) ? (1u << 3) : (1u << (3 + 16)))
#define DAP_CONFIG_SWCLK_TCK_write(val)                                        \
  (GPIOA_BSRR = (val) ? (1u << 4) : (1u << (4 + 16)))
#define DAP_CONFIG_TDO_write(val)                                              \
  (GPIOA_BSRR = (val) ? (1u << 5) : (1u << (5 + 16)))
#define DAP_CONFIG_TDI_write(val)                                              \
  (GPIOA_BSRR = (val) ? (1u << 6) : (1u << (6 + 16)))
#define DAP_CONFIG_nRESET_write(val)                                           \
  (GPIOA_BSRR = (val) ? (1u << 7) : (1u << (7 + 16)))
static inline void DAP_CONFIG_nTRST_write(int value) { (void)value; }

// --- Pin Read Macros (IDR) ---
#define DAP_CONFIG_SWDIO_TMS_read() ((GPIOA_IDR >> 3) & 1)
#define DAP_CONFIG_SWCLK_TCK_read() ((GPIOA_IDR >> 4) & 1)
#define DAP_CONFIG_TDO_read() ((GPIOA_IDR >> 5) & 1)
#define DAP_CONFIG_TDI_read() ((GPIOA_IDR >> 6) & 1)
#define DAP_CONFIG_nRESET_read() ((GPIOA_IDR >> 7) & 1)
#define DAP_CONFIG_nTRST_read() (1) // Unused

// --- Fast Clock Toggle (PA4) ---
#define DAP_CONFIG_SWCLK_TCK_set() (GPIOA_BSRR = (1u << 4))
#define DAP_CONFIG_SWCLK_TCK_clr() (GPIOA_BSRR = (1u << (4 + 16)))

// --- SWDIO Direction Control (PA3: bits 7:6 in MODER) ---
static inline void DAP_CONFIG_SWDIO_TMS_in(void) {
  GPIOA_MODER &= ~(3u << (3 * 2)); // Mode 00 = Input
}

static inline void DAP_CONFIG_SWDIO_TMS_out(void) {
  GPIOA_MODER =
      (GPIOA_MODER & ~(3u << (3 * 2))) | (1u << (3 * 2)); // Mode 01 = Output
}

// --- Setup Callback: Configure Speeds & Initial Directions ---
static inline void DAP_CONFIG_SETUP(void) {
  // 1. Set PA3, PA4, PA5, PA6, PA7 to Very High Speed (Speed 11)
  uint32_t speed = GPIOA_OSPEEDR;
  speed &= ~((3u << (3 * 2)) | (3u << (4 * 2)) | (3u << (5 * 2)) |
             (3u << (6 * 2)) | (3u << (7 * 2)));
  speed |= ((3u << (3 * 2)) | (3u << (4 * 2)) | (3u << (5 * 2)) |
            (3u << (6 * 2)) | (3u << (7 * 2)));
  GPIOA_OSPEEDR = speed;

  // 2. Set default MODER output directions for PA4 (SWCLK), PA6 (TDI), PA7
  // (nRESET)
  //    PA3 (SWDIO) and PA5 (TDO/SWO) managed dynamically or as input
  uint32_t moder = GPIOA_MODER;
  moder &=
      ~((3u << (4 * 2)) | (3u << (5 * 2)) | (3u << (6 * 2)) | (3u << (7 * 2)));
  moder |= ((1u << (4 * 2)) | (0u << (5 * 2)) | (1u << (6 * 2)) |
            (1u << (7 * 2))); // Output for 4,6,7. Input for 5.
  GPIOA_MODER = moder;

  // 3. Set nRESET high by default
  DAP_CONFIG_nRESET_write(1);
}

static inline void DAP_CONFIG_DISCONNECT(void) { DAP_CONFIG_SWDIO_TMS_in(); }

static inline void DAP_CONFIG_CONNECT_SWD(void) { DAP_CONFIG_SWDIO_TMS_out(); }

static inline void DAP_CONFIG_CONNECT_JTAG(void) { DAP_CONFIG_SWDIO_TMS_out(); }

static inline void DAP_CONFIG_LED(int index, int bit) {
  if (index == 1) {
    // PA8 (Blue LED - Target Running Status)
    // bit == 1 (ON) -> Pull LOW (Reset bit 24)
    // bit == 0 (OFF) -> Pull HIGH (Set bit 8)
    GPIOA_BSRR = (bit) ? (1u << (8 + 16)) : (1u << 8);
  }
}

static inline void DAP_CONFIG_DELAY(uint32_t cycles) {
  for (volatile uint32_t i = 0; i < cycles; i++) {
    __asm__("nop");
  }
}

#endif // DAP_CONFIG_H_
