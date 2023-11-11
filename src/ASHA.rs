use std::{str::FromStr, collections::{HashSet, hash_map::RandomState, HashMap}, error::{Error, self}, default};
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

#[derive(Clone)]
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

#[derive(Clone)]
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

#[derive(Clone)]
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

#[derive(Clone)]
struct AudioProcessor {
    readOnlyProperties: ReadOnlyProperties,
    audioStatusPoint:   u8
}

#[derive(Clone)]
pub struct DiscoveredProcessor {
    addr: bluer::Address,
    name: String,
    dc:   DeviceCapabilities
}

#[derive(Clone)]
pub enum AdapterState {
    Okay,
    NoAdapter,
    InadequateBtVersion,
    BluetoothOff
}

impl Default for AdapterState {
    fn default() -> Self { AdapterState::NoAdapter }
}

#[derive(Default, Clone)]
pub struct ASHA {
    pub state:    AdapterState,
    adapter:      Option<Adapter>,
    right:        Option<AudioProcessor>,
    left:         Option<AudioProcessor>,
    discovered:   HashMap<Address, DiscoveredProcessor>,
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
                    state:      AdapterState::NoAdapter,
                    adapter:    None,
                    right:      None,
                    left:       None,
                    discovered: HashMap::new(),
                    scan:       false
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
            right:      None,
            left:       None,
            discovered: HashMap::new(),
            scan:       false
        };
    }
    pub fn get_discovered(self) -> HashMap<Address, DiscoveredProcessor> {
        return self.discovered;
    }
    pub async fn start_scan(mut self, _count: u8){
        println!("Started scan");
        let filter = DiscoveryFilter{
            transport: bluer::DiscoveryTransport::Le,
            rssi: Some(-80),
            ..Default::default()
        };
        let adapter = match self.adapter {
            Some(adapter) => adapter,
            None                   => {
                self.scan = false;
                return;
            }
        };
        adapter.set_discovery_filter(filter).await.expect("Could not set filter");
        let discoverer = match adapter.discover_devices_with_changes().await {
            Ok(res) => res,
            Err(_) => {
                self.scan = false;
                return;
            }
        };
        pin_mut!(discoverer);

        self.scan = true;

        while self.scan {
            tokio::select! {
            Some(event) = discoverer.next() => {
                match event {
                    AdapterEvent::DeviceAdded(addr) => {
                        match self.discovered.contains_key(&addr) {
                            true => continue,
                            _    => ()
                        }
                        let dev = match adapter.device(addr) {
                            Ok(res) => res,
                            Err(_)  => continue
                        };
                        match dev.rssi().await {
                            Ok(rssi) => {
                                match rssi {
                                    Some(_) => {},
                                    None    => continue
                                }
                            },
                            Err(_)   => continue
                        }
                        let name = match dev.name().await {
                            Ok(res) => match res {
                                Some(name) => name,
                                None       => continue
                            },
                            Err(_)  => continue
                        };
                        let data = match dev.service_data().await {
                            Ok(res) => match res {
                                Some(services) => match services.get(&ASHA_UUID) {
                                    Some(matching) => matching[1],
                                    None           => continue
                                },
                                None           => continue
                            }
                            Err(_) => continue 
                        };
                        let dc = DeviceCapabilities::new(data);
                        self.discovered.insert(
                            addr,
                            DiscoveredProcessor { 
                                addr: dev.address(), 
                                name: name, 
                                dc: dc 
                            }
                        );
                    },
                    AdapterEvent::DeviceRemoved(addr) => {
                        self.discovered.remove(&addr);
                    } 
                    AdapterEvent::PropertyChanged(_) => {
                    }
                }
            }
            };
        }
    }
    pub fn stop_scan(mut self){
        self.scan = false;
    }
    pub async fn update_state(mut self){
        match self.state {
            AdapterState::Okay => return,
            _                  => {}
        }
        match self.adapter {
            Some(adapter) => {
                match adapter.is_powered().await.unwrap() {
                    true => self.state = AdapterState::Okay,
                    _    => self.state = AdapterState::BluetoothOff
                }
            }
            None                   => {
                self.state = ASHA::get_adapter_state().await;
            }
        }
    }
}