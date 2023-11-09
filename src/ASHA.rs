use std::{str::FromStr, collections::{HashSet, hash_map::RandomState}};
use futures::{pin_mut, stream::SelectAll, StreamExt};
use bluer::{monitor::{Pattern, data_type}, Session, Adapter, DiscoveryFilter, AdapterEvent, Address, Device};
use uuid::{uuid, Uuid};

const test_uuid: uuid::Uuid = uuid!("00030000-78fc-48fe-8e23-433b3a1942d0");

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

struct HiSyncID {
    manufacturer: bluer::id::Manufacturer,
    set:          uuid::Uuid
}

impl HiSyncID {
    pub fn new(data: [u8; 8]) -> HiSyncID {
        return HiSyncID{
            manufacturer: bluer::id::Manufacturer::try_from(
                ((data[0] as u8 as u16) << 8) | (data[1] as u16)
            ).unwrap(),
            set:          uuid::Builder::from_slice(
                &data[2..7]
            ).unwrap().into_uuid()
        };
    }
}

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
            hiSyncId:           HiSyncID::new(data[2..9].try_into().unwrap()),
            featureMap:         FeatureMap::new(data[10]),
            renderDelay:        ((data[11] as u8 as u16) << 8) | (data[12] as u16),
            reserved:           data[13..14].try_into().unwrap(),
            supportedCodecs:    ((data[15] as u8 as u16) << 8) | (data[16] as u16),
        };
    }
}

struct AudioProcessor {
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
    pub async fn start_scan(mut self, count: u8){
        println!("Started scan");
        let mut filter = DiscoveryFilter{
            transport: bluer::DiscoveryTransport::Le,
            rssi: Some(-90),
            ..Default::default()
        };
        // filter.uuids.insert(ASHA_UUID);
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
                        match dev.service_data().await.unwrap() {
                            Some(data) => {
                                match data.get(&ASHA_UUID.clone()) {
                                    Some(_) => { 
                                        ASHA::query_properties(dev).await;
                                    },
                                    _       => {}
                                }
                                // println!("Name is {}", serv_data.);
                            }
                            _ => {}
                        }
                    },
                    AdapterEvent::DeviceRemoved(_) | 
                    AdapterEvent::PropertyChanged(_) => {
                    }
                }
            }
            };
        }
    }
    async fn query_properties(dev: bluer::Device) -> ReadOnlyProperties {
        loop {
            tokio::select! {
                Ok(serv) = dev.service(ASHA_UUID.as_fields().1) => {
                    loop {
                        tokio::select! {
                            Ok(crt) = serv.characteristic(ROPC_UUID.as_fields().1) => {
                                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                                let data      = match crt.read().await {
                                    Ok(res)  => res,
                                    _        => continue
                                };
                                let rop       = ReadOnlyProperties::new(data.try_into().unwrap());
            
                                match rop.deviceCapabilities.side {
                                    SIDE::RIGHT => {println!("Right side!")}
                                    SIDE::LEFT  => {println!("Left side!")}
                                }
            
                                return rop;
                            }
                        }
                    }
                }
            }
        }
    }
    pub fn stop_scan(mut self){
        self.scan = false;
    }
}