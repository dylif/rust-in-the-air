use std::{thread::sleep, time::Duration};

use anyhow::{anyhow, Result};
use esp_idf_svc::hal::{modem::WifiModemPeripheral, peripheral::Peripheral};
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    hal::{
        delay,
        i2c::{I2cConfig, I2cDriver},
        prelude::*,
    },
    wifi::{ClientConfiguration, Configuration, EspWifi},
};
use sht4x::Sht4x;

#[toml_cfg::toml_config]
pub struct Config {
    #[default("INVALID")]
    mqtt_host: &'static str,
    #[default("INVALID")]
    mqtt_user: &'static str,
    #[default("INVALID")]
    mqtt_pass: &'static str,
    #[default("INVALID")]
    wifi_ssid: &'static str,
    #[default("INVALID")]
    wifi_password: &'static str,
    #[default(5)]
    start_delay_sec: u64,
    #[default(5)]
    wifi_connect_timeout_sec: u64,
}

fn connect_to_wifi<M: WifiModemPeripheral>(
    modem: impl Peripheral<P = M> + 'static,
    sys_loop: EspSystemEventLoop,
) -> Result<EspWifi<'static>> {
    let mut wifi_driver = EspWifi::new(modem, sys_loop, None).unwrap();

    wifi_driver
        .set_configuration(&Configuration::Client(ClientConfiguration {
            ssid: CONFIG
                .wifi_ssid
                .try_into()
                .map_err(|_| anyhow!("Unable to parse Wi-Fi SSID"))?,
            password: CONFIG
                .wifi_password
                .try_into()
                .map_err(|_| anyhow!("Unable to parse Wi-Fi PSK"))?,
            ..Default::default()
        }))
        .unwrap();

    wifi_driver.start()?;
    wifi_driver.connect()?;

    Ok(wifi_driver)
}

fn wait_for_condition_with_timeout(condition: impl Fn() -> bool, timeout_sec: u64) -> bool {
    for i in 1..=timeout_sec {
        if condition() {
            break;
        }
        sleep(Duration::from_secs(1));
        log::info!("{i:02}/{timeout_sec:02} second(s) elapsed");
    }

    condition()
}

fn main() -> Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("Start delay:");
    wait_for_condition_with_timeout(|| false, CONFIG.start_delay_sec);

    let peripherals = Peripherals::take().unwrap();
    let sys_loop = EspSystemEventLoop::take()?;

    // Connect to the Wi-Fi network
    let wifi = connect_to_wifi(peripherals.modem, sys_loop)?;

    if !wait_for_condition_with_timeout(
        || {
            wifi.is_connected().unwrap_or_else(|_| {
                log::warn!("Failed to get Wi-Fi connection status!");
                false
            })
        },
        CONFIG.wifi_connect_timeout_sec,
    ) {
        panic!("Failed to connect to Wi-Fi!")
    }

    let pins = peripherals.pins;
    let sda = pins.gpio41;
    let scl = pins.gpio40;
    let i2c = peripherals.i2c1;
    let config = I2cConfig::new().baudrate(100.kHz().into());
    let i2c = I2cDriver::new(i2c, sda, scl, &config)?;

    let mut delay = delay::Ets;
    let mut sht4x = Sht4x::new(i2c);

    let serial_number = sht4x
        .serial_number(&mut delay)
        .map_err(|_| anyhow!("Failed to get serial number"))?;
    log::info!("SHT4x serial number: {serial_number}");

    loop {
        let measurement = sht4x.measure(sht4x::Precision::High, &mut delay);
        if let Ok(measurement) = measurement {
            let temperature: f32 = measurement.temperature_celsius().to_num();
            let humidity: f32 = measurement.humidity_percent().to_num();

            let payload = format!("{{ temperature: {temperature}, humidity: {humidity} }}");

            log::info!("{payload}");
        }

        log::info!("Sleeping 5 secs... ");
        sleep(Duration::from_secs(5));
    }
}
