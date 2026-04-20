#![no_std]
#![no_main]

// Ensure we halt the program on panic (if we don't mention this crate it won't
// be linked)
use panic_halt as _;

// A shorter alias for the Peripheral Access Crate, which provides low-level
// register access
use rp2040_hal::{
    Sio,
    pac, pac::interrupt,
    clocks, clocks::Clock,
    Watchdog,
    adc::Adc, adc::AdcPin, 
    gpio::Pins,
    usb,
};

use embedded_hal::digital::{InputPin, OutputPin};

use usb_device::bus::UsbBusAllocator;
use usb_device::prelude::*;

mod xinput;
use xinput::{
    XINPUTClass, XinputControlReport, USB_CLASS_VENDOR, USB_DEVICE_RELEASE, USB_PROTOCOL_VENDOR,
    USB_SUBCLASS_VENDOR, USB_XINPUT_PID, USB_XINPUT_VID, XINPUT_EP_MAX_PACKET_SIZE,
};

/// The USB Device Driver (shared with the interrupt).
static mut USB_DEVICE: Option<UsbDevice<usb::UsbBus>> = None;

/// The USB Bus Driver (shared with the interrupt).
static mut USB_BUS: Option<UsbBusAllocator<usb::UsbBus>> = None;

/// The USB Human Interface Device Driver (shared with the interrupt).
static mut USB_XINPUT: Option<XINPUTClass<usb::UsbBus>> = None;

/// The linker will place this boot block at the start of our program image. We
/// need this to help the ROM bootloader get our code up and running.
/// Note: This boot block is not necessary when using a rp-hal based BSP
/// as the BSPs already perform this step.
#[link_section = ".boot2"]
#[used]
pub static BOOT2: [u8; 256] = rp2040_boot2::BOOT_LOADER_GENERIC_03H;

/// External high-speed crystal on the Raspberry Pi Pico board is 12 MHz. Adjust
/// if your board has a different frequency
const XTAL_FREQ_HZ: u32 = 12_000_000u32;

/// Entry point to our bare-metal application.
///
/// The `#[rp2040_hal::entry]` macro ensures the Cortex-M start-up code calls this function
/// as soon as all global variables and the spinlock are initialised.
///
/// The function configures the RP2040 peripherals, then toggles a GPIO pin in
/// an infinite loop. If there is an LED connected to that pin, it will blink.
#[rp2040_hal::entry]
fn main() -> ! {
    // Grab our singleton objects
    let mut pac = pac::Peripherals::take().unwrap();

    // Set up the watchdog driver - needed by the clock setup code
    let mut watchdog = Watchdog::new(pac.WATCHDOG);

    // Configure the clocks
    let clocks = clocks::init_clocks_and_plls(
        XTAL_FREQ_HZ,
        pac.XOSC,
        pac.CLOCKS,
        pac.PLL_SYS,
        pac.PLL_USB,
        &mut pac.RESETS,
        &mut watchdog,
    )
    .ok()
    .unwrap();

    // Set up the USB driver
    let usb_bus = UsbBusAllocator::new(usb::UsbBus::new(
        pac.USBCTRL_REGS,
        pac.USBCTRL_DPRAM,
        clocks.usb_clock,
        true,
        &mut pac.RESETS,
    ));
    unsafe {
        // Note (safety): This is safe as interrupts haven't been started yet
        USB_BUS = Some(usb_bus);
    }

    // Grab a reference to the USB Bus allocator. We are promising to the
    // compiler not to take mutable access to this global variable whilst this
    // reference exists!
    let bus_ref = unsafe { USB_BUS.as_ref().unwrap() };

    let usb_xinput = XINPUTClass::new(bus_ref);
    unsafe {
        // Note (safety): This is safe as interrupts haven't been started yet
        USB_XINPUT = Some(usb_xinput);
    }

    //https://pid.codes
    let usb_dev = UsbDeviceBuilder::new(bus_ref, UsbVidPid(USB_XINPUT_VID, USB_XINPUT_PID))
        .strings(&[StringDescriptors::new(LangID::EN)
        .product("Rusty Xinput gamepad")]).expect("Failed to set strings")
        // should change 16, 32,, when over report size over 8 byte ?
        .max_packet_size_0(XINPUT_EP_MAX_PACKET_SIZE as u8).expect("Failed to set max packet size")
        .device_release(USB_DEVICE_RELEASE)
        .device_protocol(USB_PROTOCOL_VENDOR)
        .device_class(USB_CLASS_VENDOR)
        .device_sub_class(USB_SUBCLASS_VENDOR)
        .build();
    unsafe {
        // Note (safety): This is safe as interrupts haven't been started yet
        USB_DEVICE = Some(usb_dev);
    }

    /*
    unsafe {
        // Enable the USB interrupt
        pac::NVIC::unmask(hal::pac::Interrupt::USBCTRL_IRQ);
    };
    */

    let core = pac::CorePeripherals::take().unwrap();
    let mut delay = cortex_m::delay::Delay::new(core.SYST, clocks.system_clock.freq().to_Hz());

    // The single-cycle I/O block controls our GPIO pins
    let sio = Sio::new(pac.SIO);

    // Set the pins to their default state
    let pins = Pins::new(
        pac.IO_BANK0,
        pac.PADS_BANK0,
        sio.gpio_bank0,
        &mut pac.RESETS,
    );

    // Enable ADC
    let mut adc = Adc::new(pac.ADC, &mut pac.RESETS);

    // Configure GPIO{26, 27, 28, 29} as an ADC input
    let mut adc_pin_0 = AdcPin::new(pins.gpio26.into_floating_input()).unwrap();
    let mut adc_pin_1 = AdcPin::new(pins.gpio27.into_floating_input()).unwrap();
    let mut adc_pin_2 = AdcPin::new(pins.gpio28.into_floating_input()).unwrap();
    let mut adc_pin_3 = AdcPin::new(pins.gpio29.into_floating_input()).unwrap();

    /*
    // NOTE:
    // RP2040-datasheet.pdf say
    // If the FIFO is full when a conversion completes, the sticky error flag FCS.OVER is set.
    // The current FIFO contents are not changed by this event,
    // but any conversion that completes whilst the FIFO is full will be lost.
    //
    // Is there always a two read interval delay?
    // After a long interval the next value to read is,
    // the next value read after a long interval is the value before that interval?
    //
    // Configure free-running mode:
    let mut adc_fifo = adc
        .build_fifo()
        // Set clock divider to target a sample rate of 1000 samples per second (1ksps).
        // The value was calculated by `(48MHz / 1ksps) - 1 = 47999.0`.
        // Please check the `clock_divider` method documentation for details.
        //.clock_divider(47999, 0)
        .clock_divider(0, 0) // default 48MHz / 96 = 500ksps
        //.set_channel(&mut adc_pin_0)
        // then alternate between GPIOS
        .round_robin((&mut adc_pin_3, &mut adc_pin_2, &mut adc_pin_1, &mut adc_pin_0))
        // Uncomment this line to produce 8-bit samples, instead of 12 bit (lower bits are discarded)
        .shift_8bit()
        // start sampling
        .start();
    */

    // Configure GPIO as an input
    let mut in_pin_r3 = pins.gpio24.into_pull_up_input();
    let mut in_pin_l3 = pins.gpio23.into_pull_up_input();
    let mut in_pin_menu = pins.gpio7.into_pull_up_input();
    let mut in_pin_overview = pins.gpio6.into_pull_up_input();
    let mut in_pin_d_down = pins.gpio18.into_pull_up_input();
    let mut in_pin_d_left = pins.gpio20.into_pull_up_input();
    let mut in_pin_d_right = pins.gpio19.into_pull_up_input();
    let mut in_pin_d_up = pins.gpio21.into_pull_up_input();
    let mut in_pin_lt = pins.gpio16.into_pull_up_input();
    let mut in_pin_lz = pins.gpio22.into_pull_up_input();
    let mut in_pin_rz = pins.gpio9.into_pull_up_input();
    let mut in_pin_rt = pins.gpio17.into_pull_up_input();
    let mut in_pin_y = pins.gpio15.into_pull_up_input();
    let mut in_pin_x = pins.gpio14.into_pull_up_input();
    let mut in_pin_b = pins.gpio13.into_pull_up_input();
    let mut in_pin_a = pins.gpio12.into_pull_up_input();

    // Configure GPIO25 as an output
    let mut led_pin = pins.gpio25.into_push_pull_output();
    led_pin.set_high().unwrap();

    // let mut toggle: bool = false;
    loop {

        // busy-wait until the FIFO contains at least 4 samples:
        // while adc_fifo.len() < 4 {}

        //led_pin.set_high().unwrap();

        // fetch 4 values from the fifo
        // let adc_result_3 = adc_fifo.read();
        // let adc_result_2 = adc_fifo.read();
        // let adc_result_1 = adc_fifo.read();
        // let adc_result_0 = adc_fifo.read();

        let adc_result_3: u16 = adc.read(&mut adc_pin_3).unwrap();
        let adc_result_2: u16 = adc.read(&mut adc_pin_2).unwrap();
        let adc_result_1: u16 = adc.read(&mut adc_pin_1).unwrap();
        let adc_result_0: u16 = adc.read(&mut adc_pin_0).unwrap();

        // u12 bit to i16 bit
        // norm is 0
        let adc_0: u16 = adc_result_0 << 4;
        let adc_1: u16 = adc_result_1 << 4;
        let adc_2: u16 = adc_result_2 << 4;
        let adc_3: u16 = adc_result_3 << 4;

        let lx: i16 = (adc_0 ^ 0b1000000000000000) as i16;
        let ly: i16 = (adc_1 ^ 0b0111111111111111) as i16;
        let rx: i16 = (adc_2 ^ 0b1000000000000000) as i16;
        let ry: i16 = (adc_3 ^ 0b0111111111111111) as i16;

        // calibrate
        let lx: i16 = lx.saturating_add(1 << 13);
        let rx: i16 = rx.saturating_add(1 << 11);

        // scale and clamp
        // * 1.5 ( 1 + 1/2 ) = 3/2
        let lx: i16 = lx.saturating_add(lx >> 1);
        let ly: i16 = ly.saturating_add(ly >> 1);
        let rx: i16 = rx.saturating_add(rx >> 1);
        let ry: i16 = ry.saturating_add(ry >> 1);

        let (mut lz, mut rz): (u8, u8) = (0, 0);
        if in_pin_lz.is_low().unwrap() {
            lz = 255;
        }
        if in_pin_rz.is_low().unwrap() {
            rz = 255;
        }

        let xinput_report = XinputControlReport {
            // byte zero
            thumb_click_right: in_pin_r3.is_low().unwrap(),
            thumb_click_left: in_pin_l3.is_low().unwrap(),
            button_view: in_pin_overview.is_low().unwrap(),
            button_menu: in_pin_menu.is_low().unwrap(),
            dpad_right: in_pin_d_right.is_low().unwrap(),
            dpad_left: in_pin_d_left.is_low().unwrap(),
            dpad_down: in_pin_d_down.is_low().unwrap(),
            dpad_up: in_pin_d_up.is_low().unwrap(),
            // byte one
            button_y: in_pin_y.is_low().unwrap(),
            button_x: in_pin_x.is_low().unwrap(),
            button_b: in_pin_b.is_low().unwrap(),
            button_a: in_pin_a.is_low().unwrap(),
            // #[packed_field(bits = "12")]
            // pub reserved: bool,
            xbox_button: false,
            shoulder_right: in_pin_rt.is_low().unwrap(),
            shoulder_left: in_pin_lt.is_low().unwrap(),
            // others
            trigger_left: lz,
            trigger_right: rz,
            js_left_x: lx,
            js_left_y: ly,
            js_right_x: rx,
            js_right_y: ry,
        };

        push_input(&xinput_report);

        unsafe {
            let usb_dev = USB_DEVICE.as_mut().unwrap();
            let usb_xinput = USB_XINPUT.as_mut().unwrap();
            usb_dev.poll(&mut [usb_xinput]);
        }
    }

    // Stop free-running mode (the returned `adc` can be reused for future captures)
    // let _adc = adc_fifo.stop();
}

/// Submit a new gamepad inpuit report to the USB stack.
///
/// We do this with interrupts disabled, to avoid a race hazard with the USB IRQ.
fn push_input(report: &XinputControlReport) -> () {
    cortex_m::interrupt::free(|_| unsafe {
        // Now interrupts are disabled, grab the global variable and, if
        // available, send it a XINPUT report
        USB_XINPUT
            .as_mut()
            .map(|xinput| xinput.write_control(report))
    })
    .unwrap()
}

/// This function is called whenever the USB Hardware generates an Interrupt
/// Request.
#[allow(non_snake_case)]
#[interrupt]
unsafe fn USBCTRL_IRQ() {
    // Handle USB request
    let usb_dev = USB_DEVICE.as_mut().unwrap();
    let usb_xinput = USB_XINPUT.as_mut().unwrap();
    usb_dev.poll(&mut [usb_xinput]);
}

// End of file
