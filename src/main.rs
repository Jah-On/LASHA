use std::{time::{Duration, Instant}, collections::HashMap, sync::Arc};

use cpal::{traits::{HostTrait, DeviceTrait, StreamTrait}, StreamConfig, BufferSize, SampleRate, Sample, FromSample};
use tokio::{time::sleep, sync::Mutex};

const SOURCE_AUDIO_CONFIG: StreamConfig = StreamConfig{
    buffer_size: BufferSize::Default,
    channels:    1,
    sample_rate: SampleRate{
        0: 16000
    }
};

/*============================== G722 CONSTANTS =============================*/
const q6: [i32; 32] =
[
    0,    35,   72,   110,  150,  190,  233,  276,
    323,  370,  422,  473,  530,  587,  650,  714,
    786,  858,  940,  1023, 1121, 1219, 1339, 1458,
    1612, 1765, 1980, 2195, 2557, 2919,    0,    0
];
const iln: [i32; 32] =
[
    0,  63, 62, 31, 30, 29, 28, 27,
    26, 25, 24, 23, 22, 21, 20, 19,
    18, 17, 16, 15, 14, 13, 12, 11,
    10,  9,  8,  7,  6,  5,  4,  0
];
const ilp: [i32; 32] =
[
    0,  61, 60, 59, 58, 57, 56, 55,
    54, 53, 52, 51, 50, 49, 48, 47,
    46, 45, 44, 43, 42, 41, 40, 39,
    38, 37, 36, 35, 34, 33, 32,  0
];
const wl: [i32; 8] =
[
    -60, -30, 58, 172, 334, 538, 1198, 3042
];
const rl42: [i32; 16] =
[
    0, 7, 6, 5, 4, 3, 2, 1, 7, 6, 5, 4, 3, 2, 1, 0
];
const ilb: [i32; 32] =
[
    2048, 2093, 2139, 2186, 2233, 2282, 2332,
    2383, 2435, 2489, 2543, 2599, 2656, 2714,
    2774, 2834, 2896, 2960, 3025, 3091, 3158,
    3228, 3298, 3371, 3444, 3520, 3597, 3676,
    3756, 3838, 3922, 4008
];
const qm4: [i32; 16] =
[
         0, -20456, -12896, -8968,
     -6288,  -4240,  -2584, -1200,
     20456,  12896,   8968,  6288,
      4240,   2584,   1200,     0
];
const qm2: [i32; 4] =
[
    -7408,  -1616,   7408,   1616
];
const qmf_coeffs: [i32; 12] =
[
       3,  -11,   12,   32, -210,  951, 3876, -805,  362, -156,   53,  -11,
];
const ihn: [i32; 3] = [0, 1, 0];
const ihp: [i32; 3] = [0, 3, 2];
const wh:  [i32; 3] = [0, -214, 798];
const rh2: [i32; 4] = [2, 1, 2, 1];

/*===========================================================================*/

type FrameArray = Arc<Mutex<Vec<Vec<u8>>>>;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut count: u8 = 0;
    // let fake_frame = [0 as u8; 160];

    let host   = cpal::default_host();
    let av_dev = host.default_input_device().unwrap();

    println!("{}", av_dev.name().unwrap());

    let frames = FrameArray::new(Mutex::new(Vec::new()));
    let cloned_frames = frames.clone();

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
        move |data, _: &_| append_frame(data, &cloned_frames), 
        move |err| {
            println!("{}", err);
        }, 
        None
    ).unwrap();

    stream.play().unwrap();

    let connected = test.get_devices_connected().await;
    println!("Peer state is {:?}", connected);
    let mut map: HashMap<ASHA::DevicesConnected, Vec<u8>> = HashMap::new();
    map.insert(connected.clone(), Vec::new());
    test.issue_start_command().await;
    println!("Start command issued!");

    let mut time_point: Instant;
    let mut data: Vec<u8>;
    while test.get_state().await == ASHA::State::Streaming {
        time_point = Instant::now();
        loop {
            sleep(Duration::from_millis(1)).await;
            data = match frames.as_ref().try_lock() {
                Ok(mut res) => match res.len() {
                    0 => continue,
                    _ => res.remove(0)
                }
                Err(_) => continue
            };
            break;
        }
        data.insert(0, count);
        *map.get_mut(&connected).unwrap() = data;
        test.send_audio_packet(
            map.clone()
        ).await;
        println!("Audio packet sent");
        test.get_device_statuses().await;
        match count {
            255 => count =  0,
            _   => count += 1
        }
        test.update_devices().await;
        while (Instant::now() - time_point) < Duration::from_millis(19) {
            sleep(Duration::from_millis(1)).await;
        }
    }
}

fn append_frame(input: &[i16], writer: &FrameArray){
    if input.len() < 320 {
        return;
    }
    let res = pcm_to_g722(input);

    loop {
        let mut guarded_vec = match writer.try_lock() {
            Ok(res) => res,
            Err(_) => continue
        };

        guarded_vec.push(res);
        break;
    }
}

fn pcm_to_g722(pcm: &[i16]) -> Vec<u8> {
    let mut return_buffer = [0 as u8; 160];

    let mut x              = [0 as i32; 24];

    let mut el:         i32;
    let mut wd:         i32;
    let mut wd1:        i32;
    let mut ril:        i32;
    let mut wd2:        i32;
    let mut il4:        i32;
    let mut ih2:        i32;
    let mut wd3:        i32;
    let mut eh:         i32;
    let mut mih:        i32;
    let mut i:          i32;
    let mut j:          usize;
    /* Low and high band PCM from the QMF */
    let mut xlow:       i32;
    let mut xhigh:      i32;
    let mut g722_bytes: i32;
    /* Even and odd tap accumulators */
    let mut sumeven:    i32;
    let mut sumodd:     i32;
    let mut ihigh:      i32;
    let mut ilow:       i32;
    let mut code:       i32;

    let mut det0:       i32 = 32;
    let mut det1:       i32 = 8;
    let mut nb0:        i32 = 0;
    let mut nb1:        i32 = 0;
    let mut out_buffer: i32 = 0;

    j          = 0;
    g722_bytes = 0;

    while g722_bytes < 160 {
        /* Apply the transmit QMF */
        /* Shuffle the buffer down */
        for i in 0..22 {
            x[i] = x[i + 2];
        }
        x[22] = pcm[j] as i32;
        j+=1;
        x[23] = pcm[j] as i32;
        j+=1;

        /* Discard every other QMF output */
        sumeven = 0;
        sumodd  = 0;
        for i in 0 .. 12 {
            sumodd  += x[2*i]     * qmf_coeffs[i];
            sumeven += x[2*i + 1] * qmf_coeffs[11 - i];
        }
        xlow  = (sumeven + sumodd) >> 14;
        xhigh = (sumeven - sumodd) >> 14;

        if (xlow & 0x0000FFFF) == xlow {
            el = xlow;
        } else 
        if xlow > 0xFFFF {
            el = 0xFFFF;
        } else {
            el = 0;
        }

        /* Block 1L, QUANTL */
        wd = match el >= 0 {
            true => el,
            _    => -(el + 1)
        };

        i = 0;
        while i < 30
        {
            wd1 = (q6[i as usize]*det0) >> 12;
            if wd < wd1 { break; }
            i+=1;
        }
        ilow = match el < 0 {
            true => iln[i as usize],
            _    => ilp[i as usize]
        };

        /* Block 2L, INVQAL */
        ril = ilow >> 2;

        /* Block 3L, LOGSCL */
        il4 = rl42[ril as usize];
        wd  = (nb0*127) >> 7;
        nb0  = wd + wl[il4 as usize];

        if nb0 < 0     { nb0 = 0;     } else 
        if nb0 > 18432 { nb0 = 18432; }

        /* Block 3L, SCALEL */
        wd1 = (nb0 >> 6) & 31;
        wd2 = 8 - (nb0 >> 11);
        wd3 = match wd2 < 0 {
            true => ilb[wd1 as usize] << -wd2,
            _    => ilb[wd1 as usize] >>  wd2
        };
        det0 = wd3 << 2;
        
        /* Block 1H, SUBTRA */
        if (xhigh & 0x0000FFFF) == xlow {
            eh = xlow;
        } else 
        if xhigh > 0xFFFF {
            eh = 0xFFFF;
        } else {
            eh = 0;
        }

        /* Block 1H, QUANTH */
        wd = match eh >= 0 {
            true => eh,
            _    => -(eh + 1)
        };
        wd1 = (564*det1) >> 12;
        mih = match wd >= wd1 {
            true => 2,
            _    => 1
        };
        ihigh = match eh < 0 {
            true => ihn[mih as usize],
            _    => ihp[mih as usize]
        };

        /* Block 3H, LOGSCH */
        ih2 = rh2[ihigh as usize];
        wd  = (nb1*127) >> 7;
        nb1  = wd + wh[ih2 as usize];
        if nb1 < 0 { nb1 = 0; } else 
        if nb1 > 22528 { nb1 = 22528; }

        /* Block 3H, SCALEH */
        wd1 = (nb1 >> 6) & 31;
        wd2 = 10 - (nb1 >> 11);
        wd3 = match wd2 < 0 {
            true => ilb[wd1 as usize] << -wd2,
            _    => ilb[wd1 as usize] >>  wd2
        };
        det1 = wd3 << 2;

        code = (ihigh << 6) | ilow;

        /* Pack the code bits */
        out_buffer |= code << 8;

        return_buffer[g722_bytes as usize] = (out_buffer & 0xFF) as u8;
        g722_bytes +=  1;

        out_buffer >>= 8;
    }

    return return_buffer.as_slice().to_owned();
}