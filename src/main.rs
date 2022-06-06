use bluer::{
    adv::Advertisement,
    adv::Type,
    gatt::{
        local::{
            characteristic_control, service_control, Application, Characteristic,
            CharacteristicControlEvent, CharacteristicNotify, CharacteristicNotifyMethod,
            CharacteristicWrite, CharacteristicWriteMethod, Service,
        },
        CharacteristicReader, CharacteristicWriter,
    },
    ErrorKind,
};
use futures::{future, pin_mut, StreamExt};
use log::error;
use log::LevelFilter;
use std::{collections::BTreeMap, time::Duration};
use syslog::{BasicLogger, Facility, Formatter3164};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    time::{interval, sleep},
};

// Standard 128 bits UUID: 0000XXXX-0000-1000-8000-00805f9b34fb

/// Human Interface Device (HID) service: 0x1812
const SERVICE_UUID: uuid::Uuid = uuid::Uuid::from_u128(0x0000181200001000800000805f9b34fb);

/// Characteristic UUID for GATT example.
const CHARACTERISTIC_UUID: uuid::Uuid = uuid::Uuid::from_u128(0xF00DC0DE00001);

/// Manufacturer id for LE advertisement.
// TODO replace with custom
#[allow(dead_code)]
const MANUFACTURER_ID: u16 = 0xf00d;
/// Keyboard appearance.
const APPEARANCE: u16 = 0x03c1;

#[tokio::main(flavor = "current_thread")]
async fn main() -> bluer::Result<()> {
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

    let session = bluer::Session::new().await?;
    let adapter = session.default_adapter().await?;
    adapter.set_powered(true).await?;

    println!(
        "Advertising on Bluetooth adapter {} with address {}",
        adapter.name(),
        adapter.address().await?
    );
    let mut manufacturer_data = BTreeMap::new();
    manufacturer_data.insert(MANUFACTURER_ID, vec![0x21, 0x22, 0x23, 0x24]);

    // TODO declare primary service HID
    let le_advertisement = Advertisement {
        advertisement_type: Type::Peripheral,
        service_uuids: vec![SERVICE_UUID].into_iter().collect(),
        manufacturer_data,
        discoverable: Some(true),
        // The keyboard appearance seems not to work when SERVICE_UUID is not the standard 0x1812.
        appearance: Some(APPEARANCE),
        // TODO use a commandline argument.
        timeout: Some(Duration::from_secs(30)),
        // TODO take the name from a command line argument.
        local_name: Some("Luc's bleyboard".to_string()),
        ..Default::default()
    };

    println!("{:?}", &le_advertisement);
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

    println!(
        "Serving GATT service on Bluetooth adapter {}",
        adapter.name()
    );
    let (service_control, service_handle) = service_control();
    let (char_control, char_handle) = characteristic_control();
    let app = Application {
        services: vec![Service {
            uuid: SERVICE_UUID,
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
        }],
        ..Default::default()
    };
    let app_handle = adapter.serve_gatt_application(app).await?;

    println!("Service handle is 0x{:x}", service_control.handle()?);
    println!("Characteristic handle is 0x{:x}", char_control.handle()?);

    println!("Service ready. Press enter to quit.");
    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();

    let mut value: Vec<u8> = vec![0x10, 0x01, 0x01, 0x10];
    let mut read_buf = Vec::new();
    let mut reader_opt: Option<CharacteristicReader> = None;
    let mut writer_opt: Option<CharacteristicWriter> = None;
    let mut interval = interval(Duration::from_secs(1));
    pin_mut!(char_control);

    loop {
        tokio::select! {
            _ = lines.next_line() => break,
            evt = char_control.next() => {
                match evt {
                    Some(CharacteristicControlEvent::Write(req)) => {
                        println!("Accepting write event with MTU {}", req.mtu());
                        read_buf = vec![0; req.mtu()];
                        reader_opt = Some(req.accept()?);
                    },
                    Some(CharacteristicControlEvent::Notify(notifier)) => {
                        println!("Accepting notify request event with MTU {}", notifier.mtu());
                        writer_opt = Some(notifier);
                    },
                    None => break,
                }
            }
            _ = interval.tick() => {
                println!("Decrementing each element by one");
                for v in &mut *value {
                    *v = v.saturating_sub(1);
                }
                println!("Value is {:x?}", &value);
                if let Some(writer) = writer_opt.as_mut() {
                    println!("Notifying with value {:x?}", &value);
                    if let Err(err) = writer.write(&value).await {
                        println!("Notification stream error: {}", &err);
                        writer_opt = None;
                    }
                }
            }
            read_res = async {
                match &mut reader_opt {
                    Some(reader) => reader.read(&mut read_buf).await,
                    None => future::pending().await,
                }
            } => {
                match read_res {
                    Ok(0) => {
                        println!("Write stream ended");
                        reader_opt = None;
                    }
                    Ok(n) => {
                        value = read_buf[0..n].to_vec();
                        println!("Write request with {} bytes: {:x?}", n, &value);
                    }
                    Err(err) => {
                        println!("Write stream error: {}", &err);
                        reader_opt = None;
                    }
                }
            }
        }
    }

    println!("Removing service and advertisement");
    drop(app_handle);
    drop(adv_handle);
    sleep(Duration::from_secs(1)).await;

    Ok(())
}
