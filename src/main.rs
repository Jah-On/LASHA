use std::{time::{Duration, Instant}, collections::HashMap, sync::{Arc, Mutex}, rc::Rc, io::Write, fs::OpenOptions};

use cpal::{traits::{HostTrait, DeviceTrait, StreamTrait}, StreamConfig, BufferSize, SampleRate, Sample, FromSample};
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

const SOURCE_AUDIO_CONFIG: StreamConfig = StreamConfig{
    buffer_size: BufferSize::Default,
    channels:    1,
    sample_rate: SampleRate{
        0: 16000
    }
};

type Frame = Arc<Mutex<Vec<i16>>>;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    // std::fs::File::create("./test.g722").unwrap();
    // let mut op = OpenOptions::new();
    // op.append(true);

    // let mut file = op.open("./test.g722").unwrap();

    let mut count: u8 = 0;

    let state = unsafe { g722_encode_init(std::ptr::null_mut(), 64000, 0) };

    let host   = cpal::default_host();
    let av_dev = host.default_input_device().unwrap();

    let frame = Frame::new(Mutex::new(Vec::new()));
    let cloned_frame = frame.clone();

    // let stream = av_dev.build_input_stream(
    //     &SOURCE_AUDIO_CONFIG, 
    //     move |data, _: &_| get_new_frame(data, &cloned_frame), 
    //     move |err| {
    //         println!("{}", err);
    //     }, 
    //     None
    // ).unwrap();

    // stream.play().unwrap();

    let mut res: Vec<u8> = Vec::new();
    // while file.metadata().unwrap().len() < 100000 {
    //     let mut data = match frame.try_lock() {
    //         Ok(res) => res,
    //         Err(_) => {
    //             std::thread::sleep(Duration::from_millis(1));
    //             continue;
    //         }
    //     };

    //     if data.len() == 0 { 
    //         std::thread::sleep(Duration::from_millis(1));
    //         continue; 
    //     }

    //     res.resize(data.len()/2, 0);

    //     unsafe {
    //         g722_encode(state, res.as_mut_slice().as_mut_ptr(), data.as_ptr(), data.len() as i32);
    //     }

    //     data.clear();

    //     file.write(&res).unwrap();
    // }
    // file.flush().unwrap();
    // stream.pause().unwrap();

    let mut test = ASHA::ASHA::new().await;

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

    let stream = av_dev.build_input_stream(
        &SOURCE_AUDIO_CONFIG, 
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
    let mut g722_data: [u8; 160] = [0; 160];

    // loop {
    //     let mut data = match frame.try_lock() {
    //         Ok(res) => res,
    //         Err(_) => {
    //             std::thread::sleep(Duration::from_millis(1));
    //             continue;
    //         }
    //     };

    //     if data.len() < 320 { 
    //         std::thread::sleep(Duration::from_millis(1));
    //         continue;
    //     }
    //     unsafe {
    //         g722_encode(state, g722_data.as_mut_ptr(), data.drain(0..320).as_slice().as_ptr(), 320);
    //     }
    //     *map.get_mut(&connected).unwrap() = g722_data.to_vec();
    //     map.get_mut(&connected).unwrap().insert(0, count);
    //     test.send_audio_packet(
    //         map.clone(), count
    //     ).await;
    //     break;
    // }
    // count += 1;
    test.issue_status_command(0).await;

    test.issue_start_command().await;

    test.get_device_statuses().await;

    while test.get_state().await == ASHA::State::Streaming {
        time_point = Instant::now();
        let mut data = match frame.try_lock() {
            Ok(res) => res,
            Err(_) => {
                std::thread::sleep(Duration::from_millis(1));
                continue;
            }
        };

        if data.len() < 320 { 
            std::thread::sleep(Duration::from_millis(1));
            continue;
        }

        // test.issue_status_command(2).await;

        unsafe {
            g722_encode(state, g722_data.as_mut_ptr(), data.drain(0..320).as_slice().as_ptr(), 320);
        }
        *map.get_mut(&connected).unwrap() = g722_data.to_vec();
        test.send_audio_packet(
            map.clone(), count
        ).await;
        // test.get_device_statuses().await;
        match count {
            255 => count  = 0,
            _   => count += 1
        }
        // test.update_devices().await;
        while (Instant::now() - time_point) < Duration::from_micros(19000) {
            sleep(Duration::from_micros(200)).await;
        }
        println!("{}", (Instant::now() - time_point).as_millis());
    }
}

fn get_new_frame(input: &[i16], frame: &Frame){
    loop {
        let mut res = match frame.try_lock() {
            Ok(res) => res,
            Err(_) => {
                std::thread::sleep(Duration::from_micros(100));
                continue;
            }
        };
        res.append(&mut input.to_vec());
        return;
    }
}
