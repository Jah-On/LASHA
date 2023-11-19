use std::{time::{Duration, Instant}, collections::HashMap, sync::{Arc, Mutex}, rc::Rc, io::Write};

use cpal::{traits::{HostTrait, DeviceTrait, StreamTrait}, StreamConfig, BufferSize, SampleRate, Sample, FromSample};
use tokio::time::sleep;

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
    buffer_size: BufferSize::Fixed(320),
    channels:    1,
    sample_rate: SampleRate{
        0: 16000
    }
};

type FrameArray = Arc<Mutex<[u8;160]>>;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    std::fs::File::create("./test.g722").unwrap();
    let mut count: u8 = 0;

    let host   = cpal::default_host();
    let av_dev = host.default_output_device().unwrap();

    println!("{}", av_dev.name().unwrap());

    let frames = FrameArray::new(Mutex::new([0;160]));
    let cloned_frames = frames.clone();

    let stream = av_dev.build_input_stream(
        &SOURCE_AUDIO_CONFIG, 
        move |data, _: &_| append_frame(data, &cloned_frames), 
        move |err| {
            println!("{}", err);
        }, 
        None
    ).unwrap();

    stream.play().unwrap();

    std::thread::sleep(Duration::from_secs(60));

    return;

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

    // let stream = av_dev.build_input_stream(
    //     &SOURCE_AUDIO_CONFIG, 
    //     move |data, _: &_| append_frame(data, &cloned_frames), 
    //     move |err| {
    //         println!("{}", err);
    //     }, 
    //     None
    // ).unwrap();

    let connected = test.get_devices_connected().await;
    let mut map: HashMap<ASHA::DevicesConnected, Vec<u8>> = HashMap::new();
    map.insert(connected.clone(), Vec::new());
    test.issue_start_command().await;
    // println!("Start command issued!");

    let mut time_point: Instant;
    let mut data: Vec<u8> = Vec::new();
    data.resize(160, 0);
    while test.get_state().await == ASHA::State::Streaming {
        time_point = Instant::now();
        data = match frames.as_ref().try_lock() {
            Ok(res) => res.to_vec(),
            Err(_) => panic!("Could not lock mutex")
        };
        data.insert(0, count);
        *map.get_mut(&connected).unwrap() = data.clone();
        test.send_audio_packet(
            map.clone()
        ).await;
        // test.get_device_statuses().await;
        match count {
            255 => count  = 0,
            _   => count += 1
        }
        test.update_devices().await;
        // println!("{}", (Instant::now() - time_point).as_millis());
        while (Instant::now() - time_point) < Duration::from_micros(19000) {
            sleep(Duration::from_micros(200)).await;
        }
    }
}

fn append_frame(input: &[i16], writer: &FrameArray){
    // println!("{:?}", input);
    // if input.len() < 320 {
    //     return;
    // }
    let g722_state: *mut g722_encode_state_t;
    unsafe {
        g722_state = g722_encode_init(std::ptr::null_mut(), 16000, 0);
    }

    let mut op = std::fs::OpenOptions::new();
    op.append(true);

    let mut file = op.open("./test.g722").unwrap();

    let mut res: Vec<u8> = Vec::new();

    unsafe {
        g722_encode(g722_state.cast(), res.as_mut_ptr(), input.as_ptr(), input.len() as i32);
    }

    file.write_all(&res).unwrap();

    if file.metadata().unwrap().len() > 100000 {
        file.flush().unwrap();
        std::process::exit(0);
    }
    // let mut guarded_vec = match writer.try_lock() {
    //     Ok(res) => res,
    //     Err(_) => panic!("Could not get lock")
    // };

    // guarded_vec.swap_with_slice(res.as_mut_slice());
}
