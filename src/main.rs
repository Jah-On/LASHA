use std::{time::{Duration, Instant}, future, default, rc::{self, Rc}, sync::{Arc, Mutex}, pin::Pin, task::Context, collections::HashMap};

use ffmpeg_next::codec::Audio;
use jack::{ClientOptions, AudioIn, ProcessScope, MidiIn};
use tokio::time::sleep;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let count: u8 = 0;
    let fake_frame = [0 as u8; 160];

    // let client = jack::Client::new("LASHA", ClientOptions::empty()).unwrap();
    // client.0.set_buffer_size(320).unwrap();
    // let port  = client.0.register_port("ASHA", MidiIn).unwrap();

    // let ps = unsafe {
    //     ProcessScope::from_raw(320, client.0.raw())
    // };

    // let audio = port.as_slice(&ps);

    let mut test = ASHA::ASHA::new().await;

    loop {
        match test.get_state().await {
            ASHA::State::NoAdapter |
            ASHA::State::InadequateBtVersion => {
                println!("No or incompatible adapter found... exiting!");
                break;
            },
            ASHA::State::BluetoothOff        => {
                println!("Bluetooth off...");
                sleep(Duration::from_millis(5000)).await;
                test.update_state().await;
                continue;
            }
            _                                       => {
                break;
            }
        }
    }

    loop {
        match test.get_devices_connected().await {
            ASHA::DevicesConnected::NONE => {
                sleep(Duration::from_millis(100)).await;
                test.update_devices().await;
                continue;
            }
            _ => {
                break;
            }
        }
    }

    let connected = test.get_devices_connected().await;
    let mut map: HashMap<ASHA::DevicesConnected, Vec<u8>> = HashMap::new();
    map.insert(connected.clone(), Vec::new());
    test.issue_start_command().await;

    let mut time_point: Instant;
    while test.get_state().await == ASHA::State::Streaming {
        time_point = Instant::now();
        let mut data = Vec::from(fake_frame);
        data.insert(0, count);
        *map.get_mut(&connected).unwrap() = data;
        test.send_audio_packet(
            map.clone()
        ).await;
        test.update_devices().await;
        while (Instant::now() - time_point) < Duration::from_millis(19) {
            sleep(Duration::from_millis(1)).await;
        }
    }
}