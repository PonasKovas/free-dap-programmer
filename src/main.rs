#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_stm32::Config;
use embassy_stm32::Peri;
use embassy_stm32::bind_interrupts;
use embassy_stm32::gpio::Input;
use embassy_stm32::gpio::Output;
use embassy_stm32::gpio::Pull;
use embassy_stm32::gpio::{Level, OutputOpenDrain, Speed};
use embassy_stm32::peripherals::USB;
use embassy_stm32::peripherals::*;
use embassy_stm32::rcc::*;
use embassy_usb::Handler;
use embassy_usb::driver::{Endpoint, EndpointIn, EndpointOut};
use embassy_usb::types::StringIndex;
use panic_halt as _;
use static_cell::StaticCell;

// C FFI bindings to free-dap
unsafe extern "C" {
    fn dap_init();
    fn dap_process_request(req: *const u8, req_size: i32, resp: *mut u8, resp_size: i32) -> i32;
}

bind_interrupts!(struct Irqs {
    USB => embassy_stm32::usb::InterruptHandler<USB>;
});

pub struct FreeDapGuard {
    _swdio: Output<'static>,
    _swclk: Output<'static>,
    _tdo: Input<'static>,
    _tdi: Output<'static>,
    _nreset: OutputOpenDrain<'static>,
    _blue_led: OutputOpenDrain<'static>,
}

impl FreeDapGuard {
    /// Takes ownership of MCU peripherals and initializes GPIO speed, directions,
    /// and default levels before invoking C dap_init().
    pub fn init(
        pa3: Peri<'static, PA3>,
        pa4: Peri<'static, PA4>,
        pa5: Peri<'static, PA5>,
        pa6: Peri<'static, PA6>,
        pa7: Peri<'static, PA7>,
        pa8: Peri<'static, PA8>,
    ) -> Self {
        // 1. Configure target pins (Speed::VeryHigh for crisp square waves)
        let swdio = Output::new(pa3, Level::High, Speed::VeryHigh);
        let swclk = Output::new(pa4, Level::Low, Speed::VeryHigh);
        let tdo = Input::new(pa5, Pull::None);
        let tdi = Output::new(pa6, Level::Low, Speed::VeryHigh);
        let nreset = OutputOpenDrain::new(pa7, Level::High, Speed::VeryHigh);

        // 2. Configure status LEDs (Level::High = OFF for Open-Drain LEDs)
        let blue_led = OutputOpenDrain::new(pa8, Level::High, Speed::Low);

        // 3. Initialize free-dap C library
        unsafe {
            dap_init();
        }

        Self {
            _swdio: swdio,
            _swclk: swclk,
            _tdo: tdo,
            _tdi: tdi,
            _nreset: nreset,
            _blue_led: blue_led,
        }
    }
}

pub struct DapStringHandler {
    pub str_index: StringIndex,
}

impl Handler for DapStringHandler {
    fn get_string(&mut self, index: StringIndex, _lang_id: u16) -> Option<&str> {
        if index == self.str_index {
            Some("CMSIS-DAP v2 Interface") // String required by probe-rs & OpenOCD
        } else {
            None
        }
    }
}

struct CmsisDapClass<'d, D: embassy_usb::driver::Driver<'d>> {
    ep_out: D::EndpointOut,
    ep_in: D::EndpointIn,
}

impl<'d, D: embassy_usb::driver::Driver<'d>> CmsisDapClass<'d, D> {
    pub fn new(builder: &mut embassy_usb::Builder<'d, D>) -> (Self, StringIndex) {
        // 1. Allocate string index FIRST (before borrowing builder for func)
        let str_idx = builder.string();

        // 2. Now create function builder
        let mut func = builder.function(0xFF, 0x00, 0x00);
        let mut iface = func.interface();
        let mut alt = iface.alt_setting(0xFF, 0x00, 0x00, Some(str_idx));

        let ep_out = alt.endpoint_bulk_out(None, 64);
        let ep_in = alt.endpoint_bulk_in(None, 64);

        (Self { ep_out, ep_in }, str_idx)
    }

    pub async fn run(mut self) -> ! {
        let mut rx_buf = [0u8; 64];
        let mut tx_buf = [0u8; 64];

        loop {
            match self.ep_out.read(&mut rx_buf).await {
                Ok(n) if n > 0 => {
                    let resp_len = unsafe {
                        dap_process_request(
                            rx_buf.as_ptr(),
                            n as i32,
                            tx_buf.as_mut_ptr(),
                            tx_buf.len() as i32,
                        )
                    };
                    if resp_len > 0 {
                        let _ = self.ep_in.write(&tx_buf[..resp_len as usize]).await;
                    }
                }
                Ok(_) => {}
                Err(_) => {
                    // USB host reset or disconnected; wait for host to re-enable endpoint
                    self.ep_out.wait_enabled().await;
                }
            }
        }
    }
}

#[embassy_executor::task]
async fn usb_task(
    mut usb: embassy_usb::UsbDevice<'static, embassy_stm32::usb::Driver<'static, USB>>,
) -> ! {
    usb.run().await
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    // (4 MHz MSI / 1 prediv) * 40 mul / 2 divr = 80 MHz System Clock
    let mut config = Config::default();
    {
        config.rcc.msi = Some(MSIRange::RANGE4M); // 4 MHz internal MSI
        config.rcc.pll = Some(Pll {
            source: PllSource::MSI,
            prediv: PllPreDiv::DIV1,
            mul: PllMul::MUL40,
            divp: None,
            divq: None,
            divr: Some(PllRDiv::DIV2),
        });
        config.rcc.sys = Sysclk::PLL1_R; // Switch SYSCLK to PLL output

        config.rcc.hsi48 = Some(Hsi48Config {
            sync_from_usb: true,
        });
    }

    let p = embassy_stm32::init(config);

    let _red_led = OutputOpenDrain::new(p.PA9, Level::Low, Speed::Low);

    let _free_dap = FreeDapGuard::init(p.PA3, p.PA4, p.PA5, p.PA6, p.PA7, p.PA8);
    core::mem::forget(_free_dap);

    // Setup USB Driver on PA11 / PA12
    let usb_driver = embassy_stm32::usb::Driver::new(p.USB, Irqs, p.PA12, p.PA11);

    // 0x1209:0x0001 = Official Open Source Hardware VID/PID
    let mut usb_config = embassy_usb::Config::new(0x1209, 0x0001);
    usb_config.manufacturer = Some("N*GGERS CORP.");
    usb_config.product = Some("CMSIS-DAP v2 Programmer");
    usb_config.serial_number = Some(embassy_stm32::uid::uid_hex());
    usb_config.max_power = 100;

    static CONFIG_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
    static BOS_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
    static MSOS_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
    static CONTROL_BUF: StaticCell<[u8; 64]> = StaticCell::new();

    let mut builder = embassy_usb::Builder::new(
        usb_driver,
        usb_config,
        CONFIG_DESCRIPTOR.init([0; 256]),
        BOS_DESCRIPTOR.init([0; 256]),
        MSOS_DESCRIPTOR.init([0; 256]),
        CONTROL_BUF.init([0; 64]),
    );

    let (dap_class, str_idx) = CmsisDapClass::new(&mut builder);

    // Attach string descriptor handler to USB builder
    static HANDLER: StaticCell<DapStringHandler> = StaticCell::new();
    let handler = HANDLER.init(DapStringHandler { str_index: str_idx });
    builder.handler(handler);

    spawner.spawn(usb_task(builder.build())).unwrap();

    dap_class.run().await;
}
