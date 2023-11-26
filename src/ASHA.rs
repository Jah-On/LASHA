use std::{
    collections::HashMap, borrow::BorrowMut,
};
use bluer::{
    Session, Adapter, Address, Device, 
    l2cap::{Socket, SocketAddr, Stream}, gatt::remote::{Characteristic, Service}, DeviceEvent
};
use tokio::io::AsyncWriteExt;
use uuid::uuid;

pub const ASHA_UUID: uuid::Uuid = uuid!("0000FDF0-0000-1000-8000-00805F9B34FB"); // ASHA Service (0xFDF0)
pub const ROPC_UUID: uuid::Uuid = uuid!("6333651e-c481-4a3e-9169-7c902aad37bb"); // Read Only Properties  characteristic
pub const ACPC_UUID: uuid::Uuid = uuid!("f0d4de7e-4a88-476c-9d9f-1937b0996cc0"); // Audio Control Point   characteristic
pub const ASTC_UUID: uuid::Uuid = uuid!("38663f1a-e711-4cac-b641-326b56404837"); // Audio Status          characteristic
pub const VOLC_UUID: uuid::Uuid = uuid!("00e4ca9e-ab14-41e4-8823-f9e70c7e91df"); // Volume                characteristic
pub const PSMC_UUID: uuid::Uuid = uuid!("2d410339-82b6-42aa-b34e-e2e01df8cc1a"); // LE Pulse Module Sense characteristic

// const ASHA_ID: u16 = 117;
// const ROPC_ID: u16 = 118;
// const ACPC_ID: u16 = 120;
// const ASTC_ID: u16 = 122;
// const VOLC_ID: u16 = 125;
// const PSMC_ID: u16 = 127;

const START_PACKET: [u8; 5] = [
    0x01, 0x01, 0x00, 0b10000000, 0x00
];

const STOP_PACKET: [u8; 1] = [
    0x02
];

#[derive(Clone, Debug)]
pub enum SIDE {
    LEFT,
    RIGHT
}


#[derive(Clone, Debug)]
pub enum MODALITY {
    MONAURAL,
    BINAURAL
}

#[derive(Eq, Hash, PartialEq, Clone, Debug)]
pub enum DevicesConnected {
    NONE,
    LEFT,
    RIGHT,
    BOTH
}

#[derive(Clone, Debug)]
pub struct DeviceCapabilities {
    side:     SIDE,
    modality: MODALITY,
    csis:     bool
}

impl DeviceCapabilities {
    fn new(data: u8) -> DeviceCapabilities {
        return DeviceCapabilities{
            side:     DeviceCapabilities::get_side(data),
            modality: DeviceCapabilities::get_modality(data),
            csis:     DeviceCapabilities::has_csis(data)
        }
    }
    fn get_side(data: u8) -> SIDE {
        match data & 0x01 {
            0 => SIDE::LEFT,
            _ => SIDE::RIGHT
        }
    }
    fn get_modality(data: u8) -> MODALITY {
        match data >> 1 & 0x01 {
            0 => MODALITY::MONAURAL,
            _ => MODALITY::BINAURAL
        }
    }
    fn has_csis(data: u8) -> bool {
        return (data >> 2 & 0x01) == 1;
    }
}

#[derive(Clone, Debug)]
struct HiSyncID {
    manufacturer: bluer::id::Manufacturer,
    set:          u64
}

impl HiSyncID {
    pub fn new(data: [u8; 8]) -> HiSyncID {
        return HiSyncID{
            manufacturer: bluer::id::Manufacturer::try_from(
                u16::from_le_bytes(data[0..2].try_into().unwrap())
            ).unwrap(),
            set:          u64::from_be_bytes(
                data[0..8].try_into().unwrap()
            ) & 0x00FFFFFF
        };
    }
}

#[derive(Clone, Debug)]
struct FeatureMap {
    coc: bool
}

impl FeatureMap {
    pub fn new(data: u8) -> FeatureMap {
        return FeatureMap{ 
            coc: (data & 0x01) == 1 
        }
    }
}

#[derive(Clone, Debug)]
struct ReadOnlyProperties {
    version:            u8,
    deviceCapabilities: DeviceCapabilities,
    hiSyncId:           HiSyncID,
    featureMap:         FeatureMap,
    renderDelay:        u16,
    reserved:           [u8; 2],
    supportedCodecs:    u16
}

impl ReadOnlyProperties {
    pub fn new(data: [u8; 17]) -> ReadOnlyProperties {
        return ReadOnlyProperties{
            version:            data[0],
            deviceCapabilities: DeviceCapabilities::new(data[1]),
            hiSyncId:           HiSyncID::new(data[2..10].try_into().unwrap()),
            featureMap:         FeatureMap::new(data[10]),
            renderDelay:        u16::from_le_bytes(
                data[11..13].try_into().unwrap()
            ),
            reserved:           data[13..15].try_into().unwrap(),
            supportedCodecs:    u16::from_le_bytes(
                data[15..17].try_into().unwrap()
            ),
        };
    }
}

// #[derive(Clone)]
struct AudioProcessor {
    //device_handle:        Device,
    service:              Service,
    read_only_properties: ReadOnlyProperties,
    audio_status_point:   Characteristic,
    audio_control_point:  Characteristic,
    volume:               Characteristic,
    socket:               Stream, 
}

// For possible feature implementation 
// #[derive(Clone)]
// pub struct DiscoveredProcessor {
//     addr: bluer::Address,
//     name: String,
//     dc:   DeviceCapabilities
// }

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    Idle,
    NoAdapter,
    InadequateBtVersion,
    BluetoothOff,
    Streaming
}

impl Default for State {
    fn default() -> Self { State::NoAdapter }
}

impl Default for DevicesConnected {
    fn default() -> Self { DevicesConnected::NONE }
}

#[derive(Default)]
pub struct ASHA {
    state:           State,
    adapter:         Option<Adapter>,
    peers_connected: HashMap<DevicesConnected, AudioProcessor>,
    addresses:       Vec<Address>
}

impl ASHA {
    pub async fn get_adapter_state() -> State {
        let temp_ses = match Session::new().await {
            Ok(res) => res,
            Err(_) => return State::NoAdapter
        };
        let temp_adapter = match temp_ses.default_adapter().await {
            Ok(res) => res,
            Err(_) => return State::NoAdapter
        };
        return match temp_adapter.is_powered().await {
            Ok(res)  => {
                match res {
                    true => State::Idle,
                    _    => State::BluetoothOff
                }
            },
            Err(_) => State::NoAdapter
        };
    }
    
    pub async fn new() -> ASHA {
        let temp_state = Self::get_adapter_state().await;
        match temp_state {
            State::NoAdapter | State::InadequateBtVersion => {
                return ASHA{
                    state:         temp_state,
                    adapter:       None,
                    ..Default::default()
                };
            },
            _                                 => ()
        }
        let temp_ses: Session = match Session::new().await {
            Ok(res) => res,
            Err(_)           => panic!("BlueZ stopped?")
        };
        return ASHA {
            state:   temp_state,
            adapter: match temp_ses.default_adapter().await {
                Ok(res) => Some(res),
                Err(_)           => panic!("Adapter disconnected during creation?")
            },
            ..Default::default()
        };
    }
    
    pub async fn get_state(& mut self) -> State {
        return self.state.to_owned();
    }
    
    pub async fn get_devices_connected(& mut self) -> DevicesConnected {
        match self.peers_connected.keys().last() {
            Some(res) => res.to_owned(),
            None      => DevicesConnected::NONE
        }
    }
    
    pub async fn update_state(&mut self){
        match self.state {
            State::Streaming => (),
            _                => self.state = ASHA::get_adapter_state().await
        }
    }
    
    pub async fn update_devices(&mut self){
        // May be updated to allow new devices during stream
        // but omitting for now for simplicity
        match self.peers_connected.len() {
            0 => self.update_from_paired().await,
            1 => self.update_from_one_connected().await,
            2 => self.update_from_two_connected().await,
            _ => return
        }
    }
    
    async fn update_from_paired(&mut self){
        let adapter = match &self.adapter {
            Some(adapter) => adapter,
            None => return
        };
        let addresses = match adapter.device_addresses().await {
            Ok(res) => res,
            Err(_)      => return
        };
        for address in addresses {
            if self.addresses.contains(&address){
                continue;
            }
            let device = match adapter.device(address) {
                Ok(device) => device,
                Err(_)     => continue
            };
            match device.is_connected().await {
                Ok(res) => match res {
                    true  => (),
                    false => continue
                }
                Err(_) => continue
            }
            match match device.is_services_resolved().await {
                Ok(bool) => bool,
                Err(_) => {
                    println!("Could not get service resolution state");
                    continue;
                }
            } {
                true  => (),
                false => {
                    println!("Service(s) not resolved, skipping...");
                    continue;
                }
            }
            let mut service_map = HashMap::new();
            let services = match device.services().await {
                Ok(res) => res,
                Err(e)  => {
                    println!("Error getting service list: {}", e);
                    continue;
                }
            };
            for serv in services {
                let u = match serv.uuid().await {
                    Ok(res) => res,
                    Err(_) => continue
                };
                service_map.insert(u, serv);
            }
            if !service_map.contains_key(&ASHA_UUID) {
                println!("ASHA service UUID not found");
                continue;
            }
            let service = &service_map[&ASHA_UUID];
            let mut char_map = HashMap::new();
            let characteristics = match service.characteristics().await {
                Ok(res) => res,
                Err(_) => continue
            };
            for c in characteristics {
                let u = match c.uuid().await {
                    Ok(res) => res,
                    Err(_) => continue
                };
                char_map.insert(u, c);
            }
            if !char_map.contains_key(&ROPC_UUID) ||
               !char_map.contains_key(&PSMC_UUID) ||
               !char_map.contains_key(&ACPC_UUID) ||
               !char_map.contains_key(&ASTC_UUID) ||
               !char_map.contains_key(&VOLC_UUID) {
                continue;
               }
            let mut characteristic = &char_map[&ROPC_UUID];
            println!("Cached value: {:?}", characteristic.cached_value().await.unwrap());
            let mut data = match characteristic.read().await {
                Ok(res) => res,
                Err(_)  => {
                    println!("Could not read characteristic!");
                    continue;
                }
            };
            let rop = ReadOnlyProperties::new(
                data.try_into().unwrap()
            );

            characteristic = &char_map[&PSMC_UUID];
            data = match characteristic.read().await {
                Ok(res) => res,
                Err(_)  => continue
            };
            let psm = ((data[1] as u16) << 8) | (data[0] as u16);
    
            let socket_addr = SocketAddr{
                addr: device.address(),
                psm:  psm,
                ..Default::default()
            };

            let generic_socket = match Socket::new_stream() {
                Ok(res) => res,
                Err(_) => continue
            };

            // generic_socket.set_flow_control(bluer::l2cap::FlowControl::Le).expect("COuld not set flow control!");
            // generic_socket.set_security(bluer::l2cap::Security{
            //     level:    bluer::l2cap::SecurityLevel::Medium,
            //     key_size: 128
            // }).expect("Could not set security!");

            let processor = AudioProcessor{
                
                //device_handle:        device,
                service:              service_map.remove(&ASHA_UUID).unwrap(),
                read_only_properties: rop,
                socket:               generic_socket.connect(socket_addr).await.unwrap(),
                audio_control_point:  char_map.remove(&ACPC_UUID).unwrap(),
                audio_status_point:   char_map.remove(&ASTC_UUID).unwrap(),
                volume:               char_map.remove(&VOLC_UUID).unwrap()
            };

            // loop {
            //     match match device.events().await {
            //         Ok(res) => res,
            //         Err(_) => continue
            //     } {
            //         DeviceEvent::PropertyChanged(_) => break
            //     }
            // }

            self.addresses.push(address);
            let side = processor.read_only_properties.deviceCapabilities.side.clone();
            match side {
                SIDE::RIGHT => self.peers_connected.insert(DevicesConnected::RIGHT, processor),
                SIDE::LEFT =>  self.peers_connected.insert(DevicesConnected::LEFT, processor)
            };
        }
    }
    
    async fn update_from_one_connected(&mut self){
        self.check_side_status().await;
    }

    async fn update_from_two_connected(&mut self){
        self.check_side_status().await;
    }

    async fn check_side_status(&mut self){
        // let keys = self.peers_connected.into_keys().cloned();
        // for peer in keys {
        //     match match self.peers_connected[&peer].device_handle.is_connected().await {
        //         Ok(res) => res,
        //         Err(_) => continue
        //     } {
        //         true  => continue,
        //         false => self.peers_connected.borrow_mut().remove(&peer).unwrap()
        //     };
        // }
    }
    
    pub async fn issue_start_command(&mut self){
        for peer in &self.peers_connected {
            // let service = match peer.1.device_handle.service(ASHA_ID).await {
            //     Ok(res) => res,
            //     Err(_)  => continue
            // };
            // let characteristic = match service.characteristic(ACPC_ID).await {
            //     Ok(res) => res,
            //     Err(_) => continue
            // };
            let characteristic = &peer.1.audio_control_point;
            match characteristic.write(START_PACKET.as_slice()).await {
                Ok(_)  => (),
                Err(_) => continue
            }
        }
        self.state = State::Streaming;
    }

    pub async fn issue_status_command(&mut self, code: u8){
        for peer in &self.peers_connected {
            // let service = match peer.1.device_handle.service(ASHA_ID).await {
            //     Ok(res) => res,
            //     Err(_)  => continue
            // };
            // let characteristic = match service.characteristic(ACPC_ID).await {
            //     Ok(res) => res,
            //     Err(_) => continue
            // };
            let characteristic = &peer.1.audio_status_point;
            match characteristic.write(&[0x03, code, 20]).await {
                Ok(_)  => (),
                Err(_) => continue
            }
        }
    }

    pub async fn send_audio_packet(&mut self, mut data: HashMap<DevicesConnected, Vec<u8>>, seq: u8) {
        for dev in data.borrow_mut() {
            let len = dev.1.len() + 1;
            let peers = self.peers_connected.borrow_mut();
            let processor = peers.get_mut(dev.0).unwrap();
            let socket = processor.socket.borrow_mut();
            dev.1.insert(0, seq);                 // Sequence
            dev.1.insert(0, 0);                   // Offset
            dev.1.insert(0, len as u8);           // Length
            socket.write_all(&dev.1).await.unwrap();
            socket.flush().await.unwrap();
        }
    }

    pub async fn get_device_statuses(&mut self) -> HashMap<DevicesConnected, u8> {
        let mut ret: HashMap<DevicesConnected, u8> = HashMap::new();
        for peer in &self.peers_connected {
            let state = match peer.1.audio_status_point.read().await {
                Ok(res)  => res,
                Err(_) => continue
            };
            ret.insert(peer.0.clone(), state[0]);
        }
        return ret;
    }

    pub async fn issue_stop_command(&mut self){
        for peer in &self.peers_connected {
            // let service = match peer.1.device_handle.service(ASHA_ID).await {
            //     Ok(res) => res,
            //     Err(_)  => continue
            // };
            // let characteristic = match service.characteristic(ACPC_ID).await {
            //     Ok(res) => res,
            //     Err(_) => continue
            // };
            let characteristic = &peer.1.audio_control_point;
            match characteristic.write(STOP_PACKET.as_slice()).await {
                Ok(_)  => (),
                Err(_) => continue
            }
        }
        self.state = State::Idle;
    }

    pub async fn close_l2cap(&mut self){
        for peer in self.peers_connected.borrow_mut() {
            peer.1.socket.shutdown().await.unwrap();
        }
        self.peers_connected.clear();
    }
}