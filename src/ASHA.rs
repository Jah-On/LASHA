use std::{str::FromStr, collections::{HashSet, hash_map::RandomState, HashMap}, error::{Error, self}, default, time::Duration};
use futures::{pin_mut, stream::SelectAll, StreamExt, select, Stream, TryFutureExt};
use bluer::{monitor::{Pattern, data_type}, Session, Adapter, DiscoveryFilter, AdapterEvent, Address, Device, gatt::remote::{Service, CharacteristicReadRequest}, UuidExt};
use tokio::time::sleep;
use uuid::{uuid, Uuid};

const test_uuid: uuid::Uuid = uuid!("00030000-78fc-48fe-8e23-433b3a1942d0");

pub const ASHA_UUID: uuid::Uuid = uuid!("0000FDF0-0000-1000-8000-00805F9B34FB"); // ASHA Service (0xFDF0)
pub const ROPC_UUID: uuid::Uuid = uuid!("6333651e-c481-4a3e-9169-7c902aad37bb"); // Read Only Properties  characteristic
pub const ACPC_UUID: uuid::Uuid = uuid!("f0d4de7e-4a88-476c-9d9f-1937b0996cc0"); // Audio Control Point   characteristic
pub const ASTC_UUID: uuid::Uuid = uuid!("38663f1a-e711-4cac-b641-326b56404837"); // Audio Status          characteristic
pub const VOLC_UUID: uuid::Uuid = uuid!("00e4ca9e-ab14-41e4-8823-f9e70c7e91df"); // Volume                characteristic
pub const PSMC_UUID: uuid::Uuid = uuid!("2d410339-82b6-42aa-b34e-e2e01df8cc1a"); // LE Pulse Module Sense characteristic

const ASHA_ID: u16 = 117;
const ROPC_ID: u16 = 118;
const ACPC_ID: u16 = 120;
const ASTC_ID: u16 = 122;
const VOLC_ID: u16 = 125;
const PSMC_ID: u16 = 127;

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

#[derive(Clone, Debug)]
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

#[derive(Clone)]
struct AudioProcessor {
    device_handle:        Device,
    read_only_properties: ReadOnlyProperties,
    audio_status_point:   u8
}

#[derive(Clone)]
pub struct DiscoveredProcessor {
    addr: bluer::Address,
    name: String,
    dc:   DeviceCapabilities
}

#[derive(Clone, Debug)]
pub enum AdapterState {
    Okay,
    NoAdapter,
    InadequateBtVersion,
    BluetoothOff
}

impl Default for AdapterState {
    fn default() -> Self { AdapterState::NoAdapter }
}

impl Default for DevicesConnected {
    fn default() -> Self { DevicesConnected::NONE }
}

#[derive(Default, Clone)]
pub struct ASHA {
    state:           AdapterState,
    adapter:         Option<Adapter>,
    right:           Option<AudioProcessor>,
    left:            Option<AudioProcessor>,
    peers_connected: DevicesConnected
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
            Ok(res)  => {
                match res {
                    true => AdapterState::Okay,
                    _    => AdapterState::BluetoothOff
                }
            },
            Err(_) => AdapterState::NoAdapter
        };
    }
    pub async fn new() -> ASHA {
        let temp_state = Self::get_adapter_state().await;
        match temp_state {
            AdapterState::NoAdapter | AdapterState::InadequateBtVersion => {
                return ASHA{
                    state:         temp_state,
                    adapter:       None,
                    right:         None,
                    left:          None,
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
            right:         None,
            left:          None,
            ..Default::default()
        };
    }
    pub async fn get_state(& mut self) -> AdapterState {
        return self.state.to_owned();
    }
    pub async fn get_devices_connected(& mut self) -> DevicesConnected {
        return self.peers_connected.to_owned();
    }
    pub async fn update_state(&mut self){
        self.state = ASHA::get_adapter_state().await;
    }
    pub async fn update_devices(&mut self){
        match self.peers_connected {
            DevicesConnected::RIGHT | 
            DevicesConnected::LEFT => self.update_from_one_connected().await,
            DevicesConnected::BOTH => self.update_from_two_connected().await,
            _ => self.update_from_paired().await
        }
    }
    async fn update_from_paired(&mut self){
        let adapter = match &self.adapter {
            Some(adapter) => adapter,
            None => return
        };
        let disocvery_filter = DiscoveryFilter{
            transport: bluer::DiscoveryTransport::Le,
            ..Default::default()
        };
        adapter.set_discovery_filter(disocvery_filter).await.expect("Filter counld not be set!");
        loop {
            let event = match match adapter.discover_devices().await {
                Ok(mut res) => res.next().await,
                Err(_)      => return
            } {
                Some(event) => event,
                None        => break
            };
            let addr = match event {
                AdapterEvent::DeviceAdded(addr) => addr,
                _                               => continue
            };
            let device = match adapter.device(addr) {
                Ok(device) => device,
                Err(_)     => continue
            };
            match device.is_paired().await {
                Ok(res) => match res {
                    true  => (),
                    false => continue
                }
                Err(_) => continue
            }
            match device.is_connected().await {
                Ok(res) => match res {
                    true  => (),
                    false => continue
                }
                Err(_) => continue
            }

            match match match device.uuids().await {
                Ok(res) => res,
                Err(_)  => {
                    continue;
                }
            } {
                Some(res) => res,
                None    => {
                    continue;
                }
            }.contains(&ASHA_UUID) {
                true  => (),
                false => continue
            };

            let service = match device.service(ASHA_ID).await {
                Ok(res) => res,
                Err(_)  => continue
            };
            let characteristic = match service.characteristic(ROPC_ID).await {
                Ok(res) => res,
                Err(_)  => continue
            };
            let data = match characteristic.read().await {
                Ok(res) => res,
                Err(_)  => continue
            };
            let rop = ReadOnlyProperties::new(
                data.try_into().unwrap()
            );

            println!("{:?}", rop);
        }
    }
    async fn update_from_one_connected(&mut self){
    }
    async fn update_from_two_connected(&mut self){
    }
}