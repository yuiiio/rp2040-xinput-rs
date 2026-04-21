#![no_std]
#![no_main]

// Ensure we halt the program on panic (if we don't mention this crate it won't
// be linked)
use panic_halt as _;

// A shorter alias for the Peripheral Access Crate, which provides low-level
// register access
use rp2040_hal::{
    Sio,
    pac,
    clocks, clocks::Clock,
    Watchdog,
    adc::Adc, adc::AdcPin, 
    gpio::Pins,
    usb,
    Timer,
};

use embedded_hal::digital::{InputPin, OutputPin};

use usb_device::bus::UsbBusAllocator;
use usb_device::prelude::*;

mod xinput;
use xinput::{
    XINPUTClass, USB_CLASS_VENDOR, USB_DEVICE_RELEASE, USB_PROTOCOL_VENDOR,
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

    let timer = Timer::new(pac.TIMER, &mut pac.RESETS, &clocks);
    let core = pac::CorePeripherals::take().unwrap();
    let mut delay = cortex_m::delay::Delay::new(core.SYST, clocks.system_clock.freq().to_Hz());

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
    led_pin.set_low().unwrap();

    // ヘッダ（0x00, 0x14）を含めた20バイトの送信専用バッファ
    let mut XINPUT_REPORT_BUFFER: [u8; 20] = [
        0x00, 0x14, // Header
        0, 0,       // Buttons (u16 LSB)
        0,          // LT
        0,          // RT
        0, 0,       // LX (i16 LSB)
        0, 0,       // LY (i16 LSB)
        0, 0,       // RX (i16 LSB)
        0, 0,       // RY (i16 LSB)
        0, 0, 0, 0, 0, 0, // Reserved
    ];

    let usb_regs = unsafe { &*pac::USBCTRL_REGS::ptr() };
    // DEV_SOF 割り込みを有効化する
    unsafe {
        usb_regs.inte().modify(|_, w| w.dev_sof().set_bit());
    }

    loop {
        /*
        unsafe {
            let usb_dev = USB_DEVICE.as_mut().unwrap();
            let usb_xinput = USB_XINPUT.as_mut().unwrap();

            usb_dev.poll(&mut [usb_xinput]);
        }
        */
        // 1. SOFが来るまで poll しながら待機
        // これが「1ms周期のスタート地点」を待つ行為になる
        while !usb_regs.ints().read().dev_sof().bit_is_set() {
            unsafe {
                let usb_dev = USB_DEVICE.as_mut().unwrap();
                let usb_xinput = USB_XINPUT.as_mut().unwrap();

                usb_dev.poll(&mut [usb_xinput]);
            }
        }

        // 2. SOFを検知した瞬間にフラグをクリア
        // SOF_RDを読むことでDEV_SOFビットが自動クリアされる
        // rp2040-pac/src/usbctrl_regs/ints.rs
        //#[doc = "Field `DEV_SOF` reader - Set every time the device receives a SOF (Start of Frame) packet. Cleared by reading SOF_RD"]
        let _frame_num = usb_regs.sof_rd().read().bits(); 

        // --- 3. 入力処理と時間計測 ---
        // SOFから数μs〜数十μs以内に完了させれば、この回のポーリングに間に合う?
        let start_proc = timer.get_counter().ticks();

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

        let mut buttons: u16 = 0;
        // byte zero
        if in_pin_r3.is_low().unwrap() { buttons |= 1 << 7; }
        if in_pin_l3.is_low().unwrap() { buttons |= 1 << 6; }
        if in_pin_overview.is_low().unwrap() { buttons |= 1 << 5; }
        if in_pin_menu.is_low().unwrap() { buttons |= 1 << 4; }
        if in_pin_d_right.is_low().unwrap() { buttons |= 1 << 3; }
        if in_pin_d_left.is_low().unwrap() { buttons |= 1 << 2; }
        if in_pin_d_down.is_low().unwrap() { buttons |= 1 << 1; }
        if in_pin_d_up.is_low().unwrap() { buttons |= 1 << 0; }
        // byte one
        if in_pin_y.is_low().unwrap() { buttons |= 1 << 15; }
        if in_pin_x.is_low().unwrap() { buttons |= 1 << 14; }
        if in_pin_b.is_low().unwrap() { buttons |= 1 << 13; }
        if in_pin_a.is_low().unwrap() { buttons |= 1 << 12; }
        // #[packed_field(bits = "12")]
        // pub reserved: bool,
        // xbox_button: false,
        if in_pin_rt.is_low().unwrap() { buttons |= 1 << 9; }
        if in_pin_lt.is_low().unwrap() { buttons |= 1 << 8; }

        let btn_bytes = buttons.to_le_bytes();
        XINPUT_REPORT_BUFFER[2] = btn_bytes[0];
        XINPUT_REPORT_BUFFER[3] = btn_bytes[1];
        // others
        XINPUT_REPORT_BUFFER[4] = lz;
        XINPUT_REPORT_BUFFER[5] = rz;

        XINPUT_REPORT_BUFFER[6..8].copy_from_slice(&lx.to_le_bytes());
        XINPUT_REPORT_BUFFER[8..10].copy_from_slice(&ly.to_le_bytes());
        XINPUT_REPORT_BUFFER[10..12].copy_from_slice(&rx.to_le_bytes());
        XINPUT_REPORT_BUFFER[12..14].copy_from_slice(&ry.to_le_bytes());

        // 1. 送信前：あらかじめ BUFF_STATUS をクリアしておく
        // (EP1 IN を想定。エンドポイント番号に合わせてビット位置を調整してください)
        /*
        const EP1_IN_BIT: u32 = 1 << 2; 
        unsafe {
            usb_regs.buff_status().write(|w| w.bits(EP1_IN_BIT));
        }
        */
        unsafe {
            let usb_xinput = USB_XINPUT.as_mut().unwrap();

            if let Ok(_) = usb_xinput.write_raw(&XINPUT_REPORT_BUFFER) {
                // 送信成功。次の周期へ
            } else {
                // ホストがまだ前回のデータを取っていない
                // 周期が早すぎるか、ホスト側が忙しいので、次のpollを待つ
            }
            //let usb_dev = USB_DEVICE.as_mut().unwrap();
            //usb_dev.poll(&mut [usb_xinput]);
        }
        /*
        let end_proc = timer.get_counter().ticks();

        // 送信完了(ACK受信)を待つ (タイムアウト付き)
        while !((usb_regs.buff_status().read().bits() & EP1_IN_BIT) != 0) {
            if timer.get_counter().ticks() - end_proc > 500 {
                led_pin.set_high().ok();
                delay.delay_ms(100);
                break; 
            } // 0.5ms以上待っても来ないなら失敗
        }
        led_pin.set_low().ok();
        */
        /*
        if process_time > LEAD_TIME { // should LEAD_TIME <= 30 us
            led_pin.set_high().ok();
            delay.delay_ms(100);
        } else {
            led_pin.set_low().ok();
        }
        */
        // do nothing
    }
}
