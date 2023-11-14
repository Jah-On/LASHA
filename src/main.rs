use std::{time::Duration, future, default, rc::{self, Rc}, sync::{Arc, Mutex}, pin::Pin, task::Context};

use cpal::{traits::{HostTrait, DeviceTrait}, StreamConfig, SampleRate};
use tokio::time::sleep;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    // let host = cpal::default_host();

    // let dev = host.default_output_device().unwrap();

    // let confy = StreamConfig{
    //     sample_rate: SampleRate{
    //         0: 16000
    //     },
    //     channels:    1,
    //     buffer_size: cpal::BufferSize::Fixed(320)
    // };

    let mut test = ASHA::ASHA::new().await;

    loop {
        match test.get_state().await {
            ASHA::AdapterState::NoAdapter |
            ASHA::AdapterState::InadequateBtVersion => {
                println!("No or incompatible adapter found... exiting!");
                break;
            },
            ASHA::AdapterState::BluetoothOff        => {
                sleep(Duration::from_millis(10)).await;
                test.update_state().await;
                continue;
            }
            _                                       => ()
        }
        match test.get_devices_connected().await {
            ASHA::DevicesConnected::NONE => {
                sleep(Duration::from_millis(100)).await;
                test.update_devices().await;
                continue;
            }
            _ => {
                println!("lol");
            }
        }
    }

    // test.start_scan(1).await;

    // tokio::time::sleep(std::time::Duration::from_secs(10)).await;

    // let devs = test.get_discovered();
    // test.stop_scan();

}