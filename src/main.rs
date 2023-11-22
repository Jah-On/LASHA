use std::{time::{Duration, Instant}, collections::HashMap, sync::{Arc, Mutex}, borrow::BorrowMut};

use ASHA::DevicesConnected;
use cpal::{traits::{HostTrait, DeviceTrait, StreamTrait}, StreamConfig, BufferSize, SampleRate};
use tokio::time::sleep;

#[derive(Debug)]
#[repr(C)]
struct band_t {
    s:               i32,
    sp:              i32,
    sz:              i32,
    r:               [i32; 3],
    a:               [i32; 3],
    ap:              [i32; 3],
    p:               [i32; 3],
    d:               [i32; 7],
    b:               [i32; 7],
    bp:              [i32; 7],
    sg:              [i32; 7],
    nb:              i32,
    det:             i32
}

#[derive(Debug)]
#[repr(C)]
struct g722_encode_state_t {
    itu_test_mode:   i32,
    packed:          i32,
    eight_k:         i32,
    bits_per_sample: i32,

    x:               [i32; 24],

    band:            [band_t; 2],

    in_buffer:       u32,
    in_bits:         i32,
    out_buffer:      u32,
    out_bits:        i32,    
}

extern "C" {
    fn g722_encode_init(s: *mut g722_encode_state_t, rate: i32, options: i32) -> *mut g722_encode_state_t;
    fn g722_encode(s: *mut g722_encode_state_t, g722_data: *mut u8, amp: *const i16, len: i32) -> i32;
}

type Frames = Arc<Mutex<HashMap<DevicesConnected, Vec<i16>>>>;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut count: u8 = 0;

    let state = unsafe { g722_encode_init(std::ptr::null_mut(), 64000, 0) };

    let host   = cpal::default_host();
    let av_dev = host.default_output_device().unwrap();

    let mut test = ASHA::ASHA::new().await;

    let frame = Frames::new(Mutex::new(HashMap::new()));
    let cloned_frame = frame.clone();
    let mut g722_data: HashMap<DevicesConnected, [u8; 160]> = HashMap::new();

    loop {
        match test.get_state().await {
            ASHA::State::NoAdapter |
            ASHA::State::InadequateBtVersion => {
                println!("No or incompatible adapter found... exiting!");
                return;
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

    let devices_connected = test.get_devices_connected().await;
    loop {
        match devices_connected {
            ASHA::DevicesConnected::NONE => {
                sleep(Duration::from_millis(100)).await;
                test.update_devices().await;
                continue;
            }
            ASHA::DevicesConnected::RIGHT |
            ASHA::DevicesConnected::LEFT  => {
                frame.try_lock().unwrap().insert(
                    devices_connected.clone(), 
                    Vec::new()
                );
                g722_data.insert(devices_connected, [0 as u8; 160]);
                break;
            }
            ASHA::DevicesConnected::BOTH  => {
                frame.try_lock().unwrap().insert(
                    ASHA::DevicesConnected::LEFT, 
                    Vec::new()
                );
                frame.try_lock().unwrap().insert(
                    ASHA::DevicesConnected::RIGHT, 
                    Vec::new()
                );
                g722_data.insert(
                    ASHA::DevicesConnected::LEFT, 
                    [0 as u8; 160]
                );
                g722_data.insert(
                    ASHA::DevicesConnected::RIGHT, 
                    [0 as u8; 160]
                ); // May need to swap order
                break;
            }
        }
    }

    let stream = av_dev.build_input_stream(
        &StreamConfig{
            buffer_size: BufferSize::Default,
            channels:    g722_data.len().max(2) as u16,
            sample_rate: SampleRate{ 0: 16000 }
        }, 
        move |data, _: &_| get_new_frame(data, &cloned_frame), 
        move |err| {
            println!("{}", err);
        }, 
        None
    ).unwrap();

    stream.play().unwrap();

    let connected = test.get_devices_connected().await;
    let mut map: HashMap<ASHA::DevicesConnected, Vec<u8>> = HashMap::new();
    map.insert(connected.clone(), Vec::new());

    let mut time_point: Instant;

    test.issue_status_command(0).await;

    for res in test.get_device_statuses().await {
        match res.1 {
            0 => println!("OK!"),
            _ => panic!("Bad state {}", res.1)
        }
    }

    test.issue_start_command().await;

    for res in test.get_device_statuses().await {
        match res.1 {
            0 => println!("OK!"),
            _ => panic!("Bad state {}", res.1)
        }
    }

    while test.get_state().await == ASHA::State::Streaming {
        time_point = Instant::now();
        let mut data = match frame.try_lock() {
            Ok(res) => res,
            Err(_) => {
                std::thread::sleep(Duration::from_millis(1));
                continue;
            }
        };

        if data.values().nth(0).unwrap().len() < 320 { 
            std::thread::sleep(Duration::from_millis(1));
            continue;
        }

        // test.issue_status_command(2).await;

        for side in g722_data.borrow_mut() {
            unsafe {
                g722_encode(
                    state, 
                    side.1.as_mut_ptr(), 
                    data.get_mut(side.0).unwrap().drain(
                        0..320
                    ).as_slice().as_ptr(), 
                    320
                );
            }
            *map.get_mut(&side.0).unwrap() = side.1.to_vec();
        }
        
        test.send_audio_packet(
            map.clone(), count
        ).await;
        for res in test.get_device_statuses().await {
            match res.1 {
                0 => println!("OK!"),
                _ => panic!("Bad state {}", res.1)
            }
        }
        // test.issue_status_command(2).await;
        match count {
            255 => count  = 0,
            _   => count += 1
        }
        // test.update_devices().await;
        while (Instant::now() - time_point) < Duration::from_micros(19000) {
            sleep(Duration::from_micros(200)).await;
        }
        // println!("{}", (Instant::now() - time_point).as_millis());
        // test.issue_stop_command().await;
        // test.close_l2cap().await;

        // break;
    }
}

fn get_new_frame(input: &[i16], frame: &Frames){
    let mut start = 0;
    loop {
        let mut res = match frame.try_lock() {
            Ok(res) => res,
            Err(_) => {
                std::thread::sleep(Duration::from_micros(100));
                continue;
            }
        };
        let channels = res.len();
        for side in res.iter_mut() {
            side.1.append(
                input[start..(input.len()/channels)+start
            ].as_ref().to_vec().as_mut());
            start += input.len()/channels;
        }
        return;
    }
}
