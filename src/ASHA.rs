use std::{str::FromStr, collections::{HashSet, hash_map::RandomState}};
use futures::{pin_mut, stream::SelectAll, StreamExt};
use bluer::{monitor::{Pattern, data_type}, Session, Adapter, DiscoveryFilter, AdapterEvent, Address};
use uuid::{uuid, Uuid};

pub const ASHA_UUID: uuid::Uuid = uuid!("0000FDF0-0000-1000-8000-00805F9B34FB"); // ASHA Service (0xFDF0)
pub const ROPC_UUID: uuid::Uuid = uuid!("6333651e-c481-4a3e-9169-7c902aad37bb"); // Read Only Properties  characteristic
pub const ACPC_UUID: uuid::Uuid = uuid!("f0d4de7e-4a88-476c-9d9f-1937b0996cc0"); // Audio Control Point   characteristic
pub const ASTC_UUID: uuid::Uuid = uuid!("38663f1a-e711-4cac-b641-326b56404837"); // Audio Status          characteristic
pub const VOLC_UUID: uuid::Uuid = uuid!("00e4ca9e-ab14-41e4-8823-f9e70c7e91df"); // Volume                characteristic
pub const PSMC_UUID: uuid::Uuid = uuid!("2d410339-82b6-42aa-b34e-e2e01df8cc1a"); // LE Pulse Module Sense characteristic

pub enum SIDE {
    LEFT,
    RIGHT
}

pub enum MODALITY {
    MONAURAL,
    BINAURAL
}

struct DeviceCapabilities {
    data: u8
}

impl DeviceCapabilities {
    fn new() -> DeviceCapabilities {
        return DeviceCapabilities{
            data: 0
        }
    }
    fn getSide(self) -> SIDE {
        match self.data & 0x01 {
            0 => SIDE::LEFT,
            _ => SIDE::RIGHT
        }
    }
    fn getModality(self) -> MODALITY {
        match self.data >> 1 & 0x01 {
            0 => MODALITY::MONAURAL,
            _ => MODALITY::BINAURAL
        }
    }
    fn hasCSIS(self) -> bool {
        return (self.data >> 2 & 0x01) == 1;
    }
    fn setData(mut self, data: u8) {
        self.data = data;
    }
}

struct HiSyncID {
    manufacturer: u16,
    setID:        u32
}

struct FeatureMap {
    data: u8
}

impl FeatureMap {
    fn hasCoC(self) -> bool {
        return (self.data & 0x01) == 1;
    }
}

struct ReadOnlyProperties {
    version:            u8,
    deviceCapabilities: DeviceCapabilities,
    hiSyncId:           HiSyncID,
    featureMap:         FeatureMap,
    renderDelay:        u16, 
    reserved:           u16,
    supportedCodecs:    u16
}

struct Device {
    readOnlyProperties: ReadOnlyProperties,
    audioStatusPoint:   u8
}

pub enum AdapterState {
    Okay,
    NoAdapter,
    InadequateBtVersion,
    BluetoothOff
}

pub struct ASHA {
    state:        AdapterState,
    session:      Session,
    adapter:      Adapter,
    scan:         bool
}

impl ASHA {
    pub async fn get_adapter_state() -> AdapterState {
        let temp_ses = match Session::new().await {
            Ok(res) => res,
            Err(_) => return AdapterState::NoAdapter
        };
        let temp_adapter = match temp_ses.default_adapter().await {
            Ok(res) => res,
            Err(_) => return AdapterState::NoAdapter
        };
        return match temp_adapter.is_powered().await {
            Ok(_)  => AdapterState::Okay,
            Err(_) => AdapterState::BluetoothOff
        };
    }
    pub async fn new() -> ASHA {
        let temp_state = Self::get_adapter_state().await;
        match temp_state {
            AdapterState::NoAdapter           => panic!("Check state before making struct!"),
            AdapterState::InadequateBtVersion => panic!("Check state before making struct!"),
            _                                 => ()
        }
        let temp_ses: Session = match Session::new().await {
            Ok(res) => res,
            Err(_)           => panic!("BlueZ turned off during creation!")
        };
        return ASHA {
            session: temp_ses.to_owned(),
            adapter: match temp_ses.default_adapter().await {
                Ok(res) => res,
                Err(_)           => panic!("Adapter disconnected during creation?")
            },
            state:   temp_state,
            scan:    false
        };
    }
    pub async fn start_scan(mut self){
        println!("Started scan");
        let mut filter = DiscoveryFilter{
            transport: bluer::DiscoveryTransport::Le,
            uuids: [ASHA_UUID].try_into().unwrap(),
            ..Default::default()
        };
        self.adapter.set_discovery_filter(filter).await.expect("Could not set filter");
        let discoverer = match self.adapter.discover_devices_with_changes().await {
            Ok(res) => res,
            Err(_) => panic!("Discoverer could not start!")
        };
        pin_mut!(discoverer);

        self.scan = true;

        while self.scan {
            tokio::select! {
                Some(event) = discoverer.next() => {
                    match event {
                        AdapterEvent::DeviceAdded(addr) => {
                            let dev = self.adapter.device(addr).unwrap();
                            match dev.rssi().await.unwrap() {
                                Some(rssi) => {
                                    println!("Found {} with an RSSI of {}", dev.name().await.unwrap().unwrap(), rssi);
                                }
                                _ => ()
                            }
                        },
                        AdapterEvent::DeviceRemoved(_) | AdapterEvent::PropertyChanged(_) => {
                            println!("Something happened!")
                        }
                    }
                }
            };
        }
    }
    pub fn stop_scan(mut self){
        self.scan = false;
    }
}