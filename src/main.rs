use ansi_term::Colour::Green;
use bluer::{
    adv::Advertisement,
    adv::Type,
    gatt::local::{
        characteristic_control, service_control, Application, Characteristic, CharacteristicNotify,
        CharacteristicNotifyMethod, CharacteristicRead, CharacteristicWrite,
        CharacteristicWriteMethod, Service,
    },
    ErrorKind,
};
use indicatif::ProgressBar;
use log::error;
use log::LevelFilter;
use log::{debug, info};
use std::{collections::BTreeMap, time::Duration};
use syslog::{BasicLogger, Facility, Formatter3164};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    time::sleep,
};

// Standard 128 bits UUID: 0000XXXX-0000-1000-8000-00805f9b34fb

const SERVICE_BATTERY_UUID: uuid::Uuid = uuid::Uuid::from_u128(0x0000180f00001000800000805f9b34fb);
const SERVICE_DEVICE_INFO_UUID: uuid::Uuid =
    uuid::Uuid::from_u128(0x0000180a00001000800000805f9b34fb);
const SERVICE_SCAN_PARAMS_UUID: uuid::Uuid =
    uuid::Uuid::from_u128(0x0000181300001000800000805f9b34fb);
const SERVICE_HID_UUID: uuid::Uuid = uuid::Uuid::from_u128(0x0000181200001000800000805f9b34fb);

/// Characteristic UUID for GATT example.
const CHARACTERISTIC_UUID: uuid::Uuid = uuid::Uuid::from_u128(0xF00DC0DE00001);

/// Manufacturer id for LE advertisement.
// TODO replace with custom
#[allow(dead_code)]
const MANUFACTURER_ID: u16 = 0xf00d;
/// Keyboard appearance.
//const APPEARANCE_HID_KEYBOARD: u16 = 0x03c1;
//const APPEARANCE_HID_MOUSE: u16 = 962;
//const APPEARANCE_HID_JOYSTICK: u16 = 963;
const APPEARANCE_HID_GAMEPAD: u16 = 964;

#[tokio::main(flavor = "current_thread")]
async fn main() -> bluer::Result<()> {
    println!("Starting Bleyboard…");

    print!("Configuring syslog… ");
    let formatter = Formatter3164 {
        facility: Facility::LOG_USER,
        hostname: None,
        // TODO use argv[0]?
        process: "bleyboard".into(),
        pid: 0,
    };
    let logger = syslog::unix(formatter).expect("could not connect to syslog");
    log::set_boxed_logger(Box::new(BasicLogger::new(logger)))
        // LDA putting LevelFilter::Trace since bluez/bluer does have trace messages I need.
        .map(|()| log::set_max_level(LevelFilter::Trace))
        .unwrap();
    println!("{}", Green.bold().paint("OK"));

    let session = bluer::Session::new().await?;
    let adapter = session.default_adapter().await?;
    adapter.set_powered(true).await?;

    print!("Configuring advertisement… ");
    info!(
        "Advertising on Bluetooth adapter {} with address {}",
        adapter.name(),
        adapter.address().await?
    );
    let mut manufacturer_data = BTreeMap::new();
    // DEV
    manufacturer_data.insert(MANUFACTURER_ID, vec![0x21, 0x22, 0x23, 0x24]);
    let le_advertisement = Advertisement {
        advertisement_type: Type::Peripheral,
        service_uuids: vec![SERVICE_BATTERY_UUID, SERVICE_HID_UUID]
            .into_iter()
            .collect(),
        manufacturer_data,
        discoverable: Some(true),
        // The keyboard appearance seems not to work when SERVICE_UUID is not the standard 0x1812.
        appearance: Some(APPEARANCE_HID_GAMEPAD),
        // TODO use a commandline argument.
        // Maximum is 180 seconds. See §5.1.1.
        timeout: Some(Duration::from_secs(30)),
        // TODO take the name from a command line argument.
        local_name: Some("Luc's bleyboard".to_string()),
        ..Default::default()
    };
    debug!("{:?}", &le_advertisement);
    let adv_handle = match adapter.advertise(le_advertisement).await {
        Ok(handle) => handle,
        Err(err) => match err.kind {
            ErrorKind::InvalidLength => {
                error!("The advertising data is too long.");
                return Err(err);
            }
            ErrorKind::Failed => {
                let msg = format!("Advertising failed: {}", err.message);
                error!("{}", msg);
                println!("{}", msg);
                return Err(err);
            }
            _ => {
                error!("Unexpected error: {}", err.message);
                return Err(err);
            }
        },
    };
    println!("{}", Green.bold().paint("OK"));

    debug!(
        "Serving GATT service on Bluetooth adapter {}",
        adapter.name()
    );

    print!("Configuring HID Keyboard services\u{2026} ");
    let (service_control, service_handle) = service_control();
    let (char_control, char_handle) = characteristic_control();
    let sbattery: Service = Service {
        uuid: SERVICE_BATTERY_UUID,
        primary: true,
        characteristics: vec![Characteristic {
            uuid: CHARACTERISTIC_UUID,
            write: Some(CharacteristicWrite {
                write: true,
                write_without_response: true,
                method: CharacteristicWriteMethod::Io,
                ..Default::default()
            }),
            notify: Some(CharacteristicNotify {
                notify: true,
                method: CharacteristicNotifyMethod::Io,
                ..Default::default()
            }),
            control_handle: char_handle,
            ..Default::default()
        }],
        control_handle: service_handle,
        ..Default::default()
    };
    let sdevice_info: Service = Service {
        uuid: SERVICE_DEVICE_INFO_UUID,
        primary: true,
        characteristics: vec![
            // TODO write the constant values in these characteristics.
            Characteristic {
                // Model Number 0x2A24
                uuid: uuid::Uuid::from_u128(0x00002a2400001000800000805f9b34fb),
                read: Some(CharacteristicRead {
                    read: true,
                    ..Default::default()
                }),
                ..Default::default()
            },
            Characteristic {
                // Serial Number 0x2A25
                uuid: uuid::Uuid::from_u128(0x00002a2500001000800000805f9b34fb),
                read: Some(CharacteristicRead {
                    read: true,
                    ..Default::default()
                }),
                ..Default::default()
            },
            Characteristic {
                // Firmware Revision 0x2A26
                uuid: uuid::Uuid::from_u128(0x00002a2600001000800000805f9b34fb),
                read: Some(CharacteristicRead {
                    read: true,
                    ..Default::default()
                }),
                ..Default::default()
            },
            Characteristic {
                // Hardware Revision 0x2a27
                uuid: uuid::Uuid::from_u128(0x00002a2700001000800000805f9b34fb),
                read: Some(CharacteristicRead {
                    read: true,
                    ..Default::default()
                }),
                ..Default::default()
            },
            Characteristic {
                // Software Revision 0x2a28
                uuid: uuid::Uuid::from_u128(0x00002a2800001000800000805f9b34fb),
                read: Some(CharacteristicRead {
                    read: true,
                    ..Default::default()
                }),
                ..Default::default()
            },
            Characteristic {
                // Manufacturer 0x2a29
                uuid: uuid::Uuid::from_u128(0x00002a2900001000800000805f9b34fb),
                read: Some(CharacteristicRead {
                    read: true,
                    ..Default::default()
                }),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let sscan_params: Service = Service {
        uuid: SERVICE_SCAN_PARAMS_UUID,
        primary: false,
        // TODO set characteristics
        ..Default::default()
    };
    let shid: Service = Service {
        uuid: SERVICE_HID_UUID,
        primary: true,
        // TODO set characteristics
        ..Default::default()
    };

    let app = Application {
        services: vec![sbattery, sdevice_info, sscan_params, shid],
        ..Default::default()
    };
    let app_handle = adapter.serve_gatt_application(app).await?;
    println!("{}", Green.bold().paint("OK"));

    info!("Service handle is 0x{:x}", service_control.handle()?);
    info!("Characteristic handle is 0x{:x}", char_control.handle()?);

    println!("{}", Green.paint("HID Keyboard is ready and advertising."));
    let scanning_progression = ProgressBar::new_spinner();
    scanning_progression.enable_steady_tick(Duration::from_millis(100));
    scanning_progression.set_message(Green.paint("advertising…").to_string());

    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();

    loop {
        tokio::select! {
            _ = lines.next_line() => break,
        }
    }
    scanning_progression.finish_and_clear();

    print!("Removing service and advertisement… ");
    drop(app_handle);
    drop(adv_handle);
    sleep(Duration::from_secs(1)).await;
    println!("{}", Green.bold().paint("OK"));

    Ok(())
}
