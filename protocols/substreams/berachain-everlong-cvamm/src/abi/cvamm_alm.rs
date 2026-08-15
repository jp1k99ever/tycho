const INTERNAL_ERR: &'static str = "`ethabi_derive` internal error";
/// Contract's functions.
#[allow(dead_code, unused_imports, unused_variables)]
pub mod functions {
    use super::INTERNAL_ERR;
}
/// Contract's events.
#[allow(dead_code, unused_imports, unused_variables)]
pub mod events {
    use super::INTERNAL_ERR;
    #[derive(Debug, Clone, PartialEq)]
    pub struct CurveRetuned {
        pub concentration_wad: substreams::scalar::BigInt,
        pub span_up_wad: substreams::scalar::BigInt,
        pub span_dn_wad: substreams::scalar::BigInt,
    }
    impl CurveRetuned {
        const TOPIC_ID: [u8; 32] = [
            183u8,
            51u8,
            74u8,
            63u8,
            40u8,
            77u8,
            110u8,
            198u8,
            191u8,
            144u8,
            232u8,
            236u8,
            100u8,
            143u8,
            185u8,
            136u8,
            223u8,
            8u8,
            106u8,
            120u8,
            170u8,
            116u8,
            179u8,
            206u8,
            128u8,
            20u8,
            82u8,
            60u8,
            216u8,
            155u8,
            108u8,
            50u8,
        ];
        pub fn match_log(log: &substreams_ethereum::pb::eth::v2::Log) -> bool {
            if log.topics.len() != 1usize {
                return false;
            }
            if log.data.len() != 96usize {
                return false;
            }
            return log.topics.get(0).expect("bounds already checked").as_ref()
                == Self::TOPIC_ID;
        }
        pub fn decode(
            log: &substreams_ethereum::pb::eth::v2::Log,
        ) -> Result<Self, String> {
            let mut values = ethabi::decode(
                    &[
                        ethabi::ParamType::Uint(256usize),
                        ethabi::ParamType::Uint(256usize),
                        ethabi::ParamType::Uint(256usize),
                    ],
                    log.data.as_ref(),
                )
                .map_err(|e| format!("unable to decode log.data: {:?}", e))?;
            values.reverse();
            Ok(Self {
                concentration_wad: {
                    let mut v = [0 as u8; 32];
                    values
                        .pop()
                        .expect(INTERNAL_ERR)
                        .into_uint()
                        .expect(INTERNAL_ERR)
                        .to_big_endian(v.as_mut_slice());
                    substreams::scalar::BigInt::from_unsigned_bytes_be(&v)
                },
                span_up_wad: {
                    let mut v = [0 as u8; 32];
                    values
                        .pop()
                        .expect(INTERNAL_ERR)
                        .into_uint()
                        .expect(INTERNAL_ERR)
                        .to_big_endian(v.as_mut_slice());
                    substreams::scalar::BigInt::from_unsigned_bytes_be(&v)
                },
                span_dn_wad: {
                    let mut v = [0 as u8; 32];
                    values
                        .pop()
                        .expect(INTERNAL_ERR)
                        .into_uint()
                        .expect(INTERNAL_ERR)
                        .to_big_endian(v.as_mut_slice());
                    substreams::scalar::BigInt::from_unsigned_bytes_be(&v)
                },
            })
        }
    }
    impl substreams_ethereum::Event for CurveRetuned {
        const NAME: &'static str = "CurveRetuned";
        fn match_log(log: &substreams_ethereum::pb::eth::v2::Log) -> bool {
            Self::match_log(log)
        }
        fn decode(log: &substreams_ethereum::pb::eth::v2::Log) -> Result<Self, String> {
            Self::decode(log)
        }
    }
    #[derive(Debug, Clone, PartialEq)]
    pub struct FeeHookSet {
        pub fee_hook: Vec<u8>,
    }
    impl FeeHookSet {
        const TOPIC_ID: [u8; 32] = [
            214u8,
            226u8,
            248u8,
            12u8,
            49u8,
            254u8,
            204u8,
            253u8,
            124u8,
            137u8,
            109u8,
            105u8,
            173u8,
            153u8,
            99u8,
            135u8,
            16u8,
            33u8,
            19u8,
            31u8,
            248u8,
            79u8,
            42u8,
            152u8,
            40u8,
            180u8,
            11u8,
            173u8,
            96u8,
            221u8,
            140u8,
            180u8,
        ];
        pub fn match_log(log: &substreams_ethereum::pb::eth::v2::Log) -> bool {
            if log.topics.len() != 1usize {
                return false;
            }
            if log.data.len() != 32usize {
                return false;
            }
            return log.topics.get(0).expect("bounds already checked").as_ref()
                == Self::TOPIC_ID;
        }
        pub fn decode(
            log: &substreams_ethereum::pb::eth::v2::Log,
        ) -> Result<Self, String> {
            let mut values = ethabi::decode(
                    &[ethabi::ParamType::Address],
                    log.data.as_ref(),
                )
                .map_err(|e| format!("unable to decode log.data: {:?}", e))?;
            values.reverse();
            Ok(Self {
                fee_hook: values
                    .pop()
                    .expect(INTERNAL_ERR)
                    .into_address()
                    .expect(INTERNAL_ERR)
                    .as_bytes()
                    .to_vec(),
            })
        }
    }
    impl substreams_ethereum::Event for FeeHookSet {
        const NAME: &'static str = "FeeHookSet";
        fn match_log(log: &substreams_ethereum::pb::eth::v2::Log) -> bool {
            Self::match_log(log)
        }
        fn decode(log: &substreams_ethereum::pb::eth::v2::Log) -> Result<Self, String> {
            Self::decode(log)
        }
    }
    #[derive(Debug, Clone, PartialEq)]
    pub struct FeesCompounded {
        pub add_stable: substreams::scalar::BigInt,
        pub add_volatile: substreams::scalar::BigInt,
        pub new_kappa: substreams::scalar::BigInt,
    }
    impl FeesCompounded {
        const TOPIC_ID: [u8; 32] = [
            147u8,
            169u8,
            201u8,
            145u8,
            22u8,
            152u8,
            254u8,
            82u8,
            174u8,
            248u8,
            87u8,
            78u8,
            223u8,
            190u8,
            40u8,
            68u8,
            82u8,
            158u8,
            84u8,
            248u8,
            205u8,
            41u8,
            179u8,
            208u8,
            77u8,
            130u8,
            178u8,
            107u8,
            19u8,
            1u8,
            251u8,
            190u8,
        ];
        pub fn match_log(log: &substreams_ethereum::pb::eth::v2::Log) -> bool {
            if log.topics.len() != 1usize {
                return false;
            }
            if log.data.len() != 96usize {
                return false;
            }
            return log.topics.get(0).expect("bounds already checked").as_ref()
                == Self::TOPIC_ID;
        }
        pub fn decode(
            log: &substreams_ethereum::pb::eth::v2::Log,
        ) -> Result<Self, String> {
            let mut values = ethabi::decode(
                    &[
                        ethabi::ParamType::Uint(256usize),
                        ethabi::ParamType::Uint(256usize),
                        ethabi::ParamType::Uint(256usize),
                    ],
                    log.data.as_ref(),
                )
                .map_err(|e| format!("unable to decode log.data: {:?}", e))?;
            values.reverse();
            Ok(Self {
                add_stable: {
                    let mut v = [0 as u8; 32];
                    values
                        .pop()
                        .expect(INTERNAL_ERR)
                        .into_uint()
                        .expect(INTERNAL_ERR)
                        .to_big_endian(v.as_mut_slice());
                    substreams::scalar::BigInt::from_unsigned_bytes_be(&v)
                },
                add_volatile: {
                    let mut v = [0 as u8; 32];
                    values
                        .pop()
                        .expect(INTERNAL_ERR)
                        .into_uint()
                        .expect(INTERNAL_ERR)
                        .to_big_endian(v.as_mut_slice());
                    substreams::scalar::BigInt::from_unsigned_bytes_be(&v)
                },
                new_kappa: {
                    let mut v = [0 as u8; 32];
                    values
                        .pop()
                        .expect(INTERNAL_ERR)
                        .into_uint()
                        .expect(INTERNAL_ERR)
                        .to_big_endian(v.as_mut_slice());
                    substreams::scalar::BigInt::from_unsigned_bytes_be(&v)
                },
            })
        }
    }
    impl substreams_ethereum::Event for FeesCompounded {
        const NAME: &'static str = "FeesCompounded";
        fn match_log(log: &substreams_ethereum::pb::eth::v2::Log) -> bool {
            Self::match_log(log)
        }
        fn decode(log: &substreams_ethereum::pb::eth::v2::Log) -> Result<Self, String> {
            Self::decode(log)
        }
    }
    #[derive(Debug, Clone, PartialEq)]
    pub struct IdleDeployed {
        pub stable_added: substreams::scalar::BigInt,
        pub volatile_added: substreams::scalar::BigInt,
        pub kappa_after: substreams::scalar::BigInt,
    }
    impl IdleDeployed {
        const TOPIC_ID: [u8; 32] = [
            239u8,
            125u8,
            17u8,
            99u8,
            90u8,
            195u8,
            62u8,
            251u8,
            8u8,
            220u8,
            64u8,
            141u8,
            139u8,
            86u8,
            35u8,
            122u8,
            93u8,
            249u8,
            52u8,
            121u8,
            63u8,
            41u8,
            138u8,
            120u8,
            193u8,
            175u8,
            30u8,
            30u8,
            238u8,
            221u8,
            171u8,
            200u8,
        ];
        pub fn match_log(log: &substreams_ethereum::pb::eth::v2::Log) -> bool {
            if log.topics.len() != 1usize {
                return false;
            }
            if log.data.len() != 96usize {
                return false;
            }
            return log.topics.get(0).expect("bounds already checked").as_ref()
                == Self::TOPIC_ID;
        }
        pub fn decode(
            log: &substreams_ethereum::pb::eth::v2::Log,
        ) -> Result<Self, String> {
            let mut values = ethabi::decode(
                    &[
                        ethabi::ParamType::Uint(256usize),
                        ethabi::ParamType::Uint(256usize),
                        ethabi::ParamType::Uint(256usize),
                    ],
                    log.data.as_ref(),
                )
                .map_err(|e| format!("unable to decode log.data: {:?}", e))?;
            values.reverse();
            Ok(Self {
                stable_added: {
                    let mut v = [0 as u8; 32];
                    values
                        .pop()
                        .expect(INTERNAL_ERR)
                        .into_uint()
                        .expect(INTERNAL_ERR)
                        .to_big_endian(v.as_mut_slice());
                    substreams::scalar::BigInt::from_unsigned_bytes_be(&v)
                },
                volatile_added: {
                    let mut v = [0 as u8; 32];
                    values
                        .pop()
                        .expect(INTERNAL_ERR)
                        .into_uint()
                        .expect(INTERNAL_ERR)
                        .to_big_endian(v.as_mut_slice());
                    substreams::scalar::BigInt::from_unsigned_bytes_be(&v)
                },
                kappa_after: {
                    let mut v = [0 as u8; 32];
                    values
                        .pop()
                        .expect(INTERNAL_ERR)
                        .into_uint()
                        .expect(INTERNAL_ERR)
                        .to_big_endian(v.as_mut_slice());
                    substreams::scalar::BigInt::from_unsigned_bytes_be(&v)
                },
            })
        }
    }
    impl substreams_ethereum::Event for IdleDeployed {
        const NAME: &'static str = "IdleDeployed";
        fn match_log(log: &substreams_ethereum::pb::eth::v2::Log) -> bool {
            Self::match_log(log)
        }
        fn decode(log: &substreams_ethereum::pb::eth::v2::Log) -> Result<Self, String> {
            Self::decode(log)
        }
    }
    #[derive(Debug, Clone, PartialEq)]
    pub struct Initialized {
        pub token0: Vec<u8>,
        pub token1: Vec<u8>,
        pub anchor_pool_x96: substreams::scalar::BigInt,
    }
    impl Initialized {
        const TOPIC_ID: [u8; 32] = [
            93u8,
            35u8,
            228u8,
            166u8,
            158u8,
            102u8,
            201u8,
            144u8,
            64u8,
            154u8,
            212u8,
            84u8,
            105u8,
            34u8,
            16u8,
            183u8,
            107u8,
            213u8,
            131u8,
            48u8,
            31u8,
            235u8,
            203u8,
            24u8,
            126u8,
            36u8,
            116u8,
            168u8,
            59u8,
            93u8,
            160u8,
            185u8,
        ];
        pub fn match_log(log: &substreams_ethereum::pb::eth::v2::Log) -> bool {
            if log.topics.len() != 3usize {
                return false;
            }
            if log.data.len() != 32usize {
                return false;
            }
            return log.topics.get(0).expect("bounds already checked").as_ref()
                == Self::TOPIC_ID;
        }
        pub fn decode(
            log: &substreams_ethereum::pb::eth::v2::Log,
        ) -> Result<Self, String> {
            let mut values = ethabi::decode(
                    &[ethabi::ParamType::Uint(160usize)],
                    log.data.as_ref(),
                )
                .map_err(|e| format!("unable to decode log.data: {:?}", e))?;
            values.reverse();
            Ok(Self {
                token0: ethabi::decode(
                        &[ethabi::ParamType::Address],
                        log.topics[1usize].as_ref(),
                    )
                    .map_err(|e| {
                        format!(
                            "unable to decode param 'token0' from topic of type 'address': {:?}",
                            e
                        )
                    })?
                    .pop()
                    .expect(INTERNAL_ERR)
                    .into_address()
                    .expect(INTERNAL_ERR)
                    .as_bytes()
                    .to_vec(),
                token1: ethabi::decode(
                        &[ethabi::ParamType::Address],
                        log.topics[2usize].as_ref(),
                    )
                    .map_err(|e| {
                        format!(
                            "unable to decode param 'token1' from topic of type 'address': {:?}",
                            e
                        )
                    })?
                    .pop()
                    .expect(INTERNAL_ERR)
                    .into_address()
                    .expect(INTERNAL_ERR)
                    .as_bytes()
                    .to_vec(),
                anchor_pool_x96: {
                    let mut v = [0 as u8; 32];
                    values
                        .pop()
                        .expect(INTERNAL_ERR)
                        .into_uint()
                        .expect(INTERNAL_ERR)
                        .to_big_endian(v.as_mut_slice());
                    substreams::scalar::BigInt::from_unsigned_bytes_be(&v)
                },
            })
        }
    }
    impl substreams_ethereum::Event for Initialized {
        const NAME: &'static str = "Initialized";
        fn match_log(log: &substreams_ethereum::pb::eth::v2::Log) -> bool {
            Self::match_log(log)
        }
        fn decode(log: &substreams_ethereum::pb::eth::v2::Log) -> Result<Self, String> {
            Self::decode(log)
        }
    }
    #[derive(Debug, Clone, PartialEq)]
    pub struct PauseSet {
        pub paused: bool,
        pub by: Vec<u8>,
    }
    impl PauseSet {
        const TOPIC_ID: [u8; 32] = [
            135u8,
            106u8,
            200u8,
            113u8,
            178u8,
            116u8,
            253u8,
            230u8,
            72u8,
            92u8,
            185u8,
            55u8,
            145u8,
            49u8,
            6u8,
            74u8,
            208u8,
            249u8,
            228u8,
            116u8,
            224u8,
            85u8,
            36u8,
            115u8,
            30u8,
            162u8,
            32u8,
            61u8,
            49u8,
            61u8,
            105u8,
            32u8,
        ];
        pub fn match_log(log: &substreams_ethereum::pb::eth::v2::Log) -> bool {
            if log.topics.len() != 2usize {
                return false;
            }
            if log.data.len() != 32usize {
                return false;
            }
            return log.topics.get(0).expect("bounds already checked").as_ref()
                == Self::TOPIC_ID;
        }
        pub fn decode(
            log: &substreams_ethereum::pb::eth::v2::Log,
        ) -> Result<Self, String> {
            let mut values = ethabi::decode(
                    &[ethabi::ParamType::Bool],
                    log.data.as_ref(),
                )
                .map_err(|e| format!("unable to decode log.data: {:?}", e))?;
            values.reverse();
            Ok(Self {
                by: ethabi::decode(
                        &[ethabi::ParamType::Address],
                        log.topics[1usize].as_ref(),
                    )
                    .map_err(|e| {
                        format!(
                            "unable to decode param 'by' from topic of type 'address': {:?}",
                            e
                        )
                    })?
                    .pop()
                    .expect(INTERNAL_ERR)
                    .into_address()
                    .expect(INTERNAL_ERR)
                    .as_bytes()
                    .to_vec(),
                paused: values
                    .pop()
                    .expect(INTERNAL_ERR)
                    .into_bool()
                    .expect(INTERNAL_ERR),
            })
        }
    }
    impl substreams_ethereum::Event for PauseSet {
        const NAME: &'static str = "PauseSet";
        fn match_log(log: &substreams_ethereum::pb::eth::v2::Log) -> bool {
            Self::match_log(log)
        }
        fn decode(log: &substreams_ethereum::pb::eth::v2::Log) -> Result<Self, String> {
            Self::decode(log)
        }
    }
    #[derive(Debug, Clone, PartialEq)]
    pub struct Recentered {
        pub anchor_before: substreams::scalar::BigInt,
        pub anchor_after: substreams::scalar::BigInt,
        pub kappa_after: substreams::scalar::BigInt,
        pub x_after: substreams::scalar::BigInt,
    }
    impl Recentered {
        const TOPIC_ID: [u8; 32] = [
            57u8,
            181u8,
            219u8,
            135u8,
            235u8,
            100u8,
            16u8,
            219u8,
            143u8,
            87u8,
            15u8,
            169u8,
            189u8,
            33u8,
            152u8,
            183u8,
            191u8,
            200u8,
            250u8,
            13u8,
            52u8,
            113u8,
            38u8,
            147u8,
            148u8,
            129u8,
            55u8,
            254u8,
            13u8,
            113u8,
            0u8,
            141u8,
        ];
        pub fn match_log(log: &substreams_ethereum::pb::eth::v2::Log) -> bool {
            if log.topics.len() != 1usize {
                return false;
            }
            if log.data.len() != 128usize {
                return false;
            }
            return log.topics.get(0).expect("bounds already checked").as_ref()
                == Self::TOPIC_ID;
        }
        pub fn decode(
            log: &substreams_ethereum::pb::eth::v2::Log,
        ) -> Result<Self, String> {
            let mut values = ethabi::decode(
                    &[
                        ethabi::ParamType::Uint(160usize),
                        ethabi::ParamType::Uint(160usize),
                        ethabi::ParamType::Uint(256usize),
                        ethabi::ParamType::Uint(256usize),
                    ],
                    log.data.as_ref(),
                )
                .map_err(|e| format!("unable to decode log.data: {:?}", e))?;
            values.reverse();
            Ok(Self {
                anchor_before: {
                    let mut v = [0 as u8; 32];
                    values
                        .pop()
                        .expect(INTERNAL_ERR)
                        .into_uint()
                        .expect(INTERNAL_ERR)
                        .to_big_endian(v.as_mut_slice());
                    substreams::scalar::BigInt::from_unsigned_bytes_be(&v)
                },
                anchor_after: {
                    let mut v = [0 as u8; 32];
                    values
                        .pop()
                        .expect(INTERNAL_ERR)
                        .into_uint()
                        .expect(INTERNAL_ERR)
                        .to_big_endian(v.as_mut_slice());
                    substreams::scalar::BigInt::from_unsigned_bytes_be(&v)
                },
                kappa_after: {
                    let mut v = [0 as u8; 32];
                    values
                        .pop()
                        .expect(INTERNAL_ERR)
                        .into_uint()
                        .expect(INTERNAL_ERR)
                        .to_big_endian(v.as_mut_slice());
                    substreams::scalar::BigInt::from_unsigned_bytes_be(&v)
                },
                x_after: {
                    let mut v = [0 as u8; 32];
                    values
                        .pop()
                        .expect(INTERNAL_ERR)
                        .into_uint()
                        .expect(INTERNAL_ERR)
                        .to_big_endian(v.as_mut_slice());
                    substreams::scalar::BigInt::from_unsigned_bytes_be(&v)
                },
            })
        }
    }
    impl substreams_ethereum::Event for Recentered {
        const NAME: &'static str = "Recentered";
        fn match_log(log: &substreams_ethereum::pb::eth::v2::Log) -> bool {
            Self::match_log(log)
        }
        fn decode(log: &substreams_ethereum::pb::eth::v2::Log) -> Result<Self, String> {
            Self::decode(log)
        }
    }
    #[derive(Debug, Clone, PartialEq)]
    pub struct ReservesRetracted {
        pub stable: substreams::scalar::BigInt,
        pub volatile_amount: substreams::scalar::BigInt,
    }
    impl ReservesRetracted {
        const TOPIC_ID: [u8; 32] = [
            89u8,
            160u8,
            63u8,
            11u8,
            103u8,
            190u8,
            21u8,
            23u8,
            11u8,
            229u8,
            195u8,
            234u8,
            244u8,
            56u8,
            179u8,
            204u8,
            179u8,
            159u8,
            18u8,
            255u8,
            41u8,
            213u8,
            167u8,
            84u8,
            159u8,
            208u8,
            179u8,
            47u8,
            127u8,
            247u8,
            10u8,
            10u8,
        ];
        pub fn match_log(log: &substreams_ethereum::pb::eth::v2::Log) -> bool {
            if log.topics.len() != 1usize {
                return false;
            }
            if log.data.len() != 64usize {
                return false;
            }
            return log.topics.get(0).expect("bounds already checked").as_ref()
                == Self::TOPIC_ID;
        }
        pub fn decode(
            log: &substreams_ethereum::pb::eth::v2::Log,
        ) -> Result<Self, String> {
            let mut values = ethabi::decode(
                    &[
                        ethabi::ParamType::Uint(256usize),
                        ethabi::ParamType::Uint(256usize),
                    ],
                    log.data.as_ref(),
                )
                .map_err(|e| format!("unable to decode log.data: {:?}", e))?;
            values.reverse();
            Ok(Self {
                stable: {
                    let mut v = [0 as u8; 32];
                    values
                        .pop()
                        .expect(INTERNAL_ERR)
                        .into_uint()
                        .expect(INTERNAL_ERR)
                        .to_big_endian(v.as_mut_slice());
                    substreams::scalar::BigInt::from_unsigned_bytes_be(&v)
                },
                volatile_amount: {
                    let mut v = [0 as u8; 32];
                    values
                        .pop()
                        .expect(INTERNAL_ERR)
                        .into_uint()
                        .expect(INTERNAL_ERR)
                        .to_big_endian(v.as_mut_slice());
                    substreams::scalar::BigInt::from_unsigned_bytes_be(&v)
                },
            })
        }
    }
    impl substreams_ethereum::Event for ReservesRetracted {
        const NAME: &'static str = "ReservesRetracted";
        fn match_log(log: &substreams_ethereum::pb::eth::v2::Log) -> bool {
            Self::match_log(log)
        }
        fn decode(log: &substreams_ethereum::pb::eth::v2::Log) -> Result<Self, String> {
            Self::decode(log)
        }
    }
    #[derive(Debug, Clone, PartialEq)]
    pub struct StateSync {
        pub x_wad: substreams::scalar::BigInt,
        pub kappa: substreams::scalar::BigInt,
        pub reserve_stable: substreams::scalar::BigInt,
        pub reserve_volatile: substreams::scalar::BigInt,
        pub fee_stable_in_wad: substreams::scalar::BigInt,
        pub fee_volatile_in_wad: substreams::scalar::BigInt,
    }
    impl StateSync {
        const TOPIC_ID: [u8; 32] = [
            79u8,
            248u8,
            81u8,
            12u8,
            57u8,
            116u8,
            110u8,
            224u8,
            74u8,
            125u8,
            146u8,
            7u8,
            241u8,
            228u8,
            30u8,
            25u8,
            139u8,
            179u8,
            190u8,
            194u8,
            22u8,
            4u8,
            181u8,
            201u8,
            48u8,
            104u8,
            78u8,
            42u8,
            218u8,
            116u8,
            175u8,
            101u8,
        ];
        pub fn match_log(log: &substreams_ethereum::pb::eth::v2::Log) -> bool {
            if log.topics.len() != 1usize {
                return false;
            }
            if log.data.len() != 192usize {
                return false;
            }
            return log.topics.get(0).expect("bounds already checked").as_ref()
                == Self::TOPIC_ID;
        }
        pub fn decode(
            log: &substreams_ethereum::pb::eth::v2::Log,
        ) -> Result<Self, String> {
            let mut values = ethabi::decode(
                    &[
                        ethabi::ParamType::Uint(256usize),
                        ethabi::ParamType::Uint(256usize),
                        ethabi::ParamType::Uint(256usize),
                        ethabi::ParamType::Uint(256usize),
                        ethabi::ParamType::Uint(256usize),
                        ethabi::ParamType::Uint(256usize),
                    ],
                    log.data.as_ref(),
                )
                .map_err(|e| format!("unable to decode log.data: {:?}", e))?;
            values.reverse();
            Ok(Self {
                x_wad: {
                    let mut v = [0 as u8; 32];
                    values
                        .pop()
                        .expect(INTERNAL_ERR)
                        .into_uint()
                        .expect(INTERNAL_ERR)
                        .to_big_endian(v.as_mut_slice());
                    substreams::scalar::BigInt::from_unsigned_bytes_be(&v)
                },
                kappa: {
                    let mut v = [0 as u8; 32];
                    values
                        .pop()
                        .expect(INTERNAL_ERR)
                        .into_uint()
                        .expect(INTERNAL_ERR)
                        .to_big_endian(v.as_mut_slice());
                    substreams::scalar::BigInt::from_unsigned_bytes_be(&v)
                },
                reserve_stable: {
                    let mut v = [0 as u8; 32];
                    values
                        .pop()
                        .expect(INTERNAL_ERR)
                        .into_uint()
                        .expect(INTERNAL_ERR)
                        .to_big_endian(v.as_mut_slice());
                    substreams::scalar::BigInt::from_unsigned_bytes_be(&v)
                },
                reserve_volatile: {
                    let mut v = [0 as u8; 32];
                    values
                        .pop()
                        .expect(INTERNAL_ERR)
                        .into_uint()
                        .expect(INTERNAL_ERR)
                        .to_big_endian(v.as_mut_slice());
                    substreams::scalar::BigInt::from_unsigned_bytes_be(&v)
                },
                fee_stable_in_wad: {
                    let mut v = [0 as u8; 32];
                    values
                        .pop()
                        .expect(INTERNAL_ERR)
                        .into_uint()
                        .expect(INTERNAL_ERR)
                        .to_big_endian(v.as_mut_slice());
                    substreams::scalar::BigInt::from_unsigned_bytes_be(&v)
                },
                fee_volatile_in_wad: {
                    let mut v = [0 as u8; 32];
                    values
                        .pop()
                        .expect(INTERNAL_ERR)
                        .into_uint()
                        .expect(INTERNAL_ERR)
                        .to_big_endian(v.as_mut_slice());
                    substreams::scalar::BigInt::from_unsigned_bytes_be(&v)
                },
            })
        }
    }
    impl substreams_ethereum::Event for StateSync {
        const NAME: &'static str = "StateSync";
        fn match_log(log: &substreams_ethereum::pb::eth::v2::Log) -> bool {
            Self::match_log(log)
        }
        fn decode(log: &substreams_ethereum::pb::eth::v2::Log) -> Result<Self, String> {
            Self::decode(log)
        }
    }
    #[derive(Debug, Clone, PartialEq)]
    pub struct Swap {
        pub sender: Vec<u8>,
        pub recipient: Vec<u8>,
        pub amount0: substreams::scalar::BigInt,
        pub amount1: substreams::scalar::BigInt,
        pub sqrt_price_x96: substreams::scalar::BigInt,
        pub liquidity: substreams::scalar::BigInt,
        pub tick: substreams::scalar::BigInt,
    }
    impl Swap {
        const TOPIC_ID: [u8; 32] = [
            196u8,
            32u8,
            121u8,
            249u8,
            74u8,
            99u8,
            80u8,
            215u8,
            230u8,
            35u8,
            95u8,
            41u8,
            23u8,
            73u8,
            36u8,
            249u8,
            40u8,
            204u8,
            42u8,
            200u8,
            24u8,
            235u8,
            100u8,
            254u8,
            216u8,
            0u8,
            78u8,
            17u8,
            95u8,
            188u8,
            202u8,
            103u8,
        ];
        pub fn match_log(log: &substreams_ethereum::pb::eth::v2::Log) -> bool {
            if log.topics.len() != 3usize {
                return false;
            }
            if log.data.len() != 160usize {
                return false;
            }
            return log.topics.get(0).expect("bounds already checked").as_ref()
                == Self::TOPIC_ID;
        }
        pub fn decode(
            log: &substreams_ethereum::pb::eth::v2::Log,
        ) -> Result<Self, String> {
            let mut values = ethabi::decode(
                    &[
                        ethabi::ParamType::Int(256usize),
                        ethabi::ParamType::Int(256usize),
                        ethabi::ParamType::Uint(160usize),
                        ethabi::ParamType::Uint(128usize),
                        ethabi::ParamType::Int(24usize),
                    ],
                    log.data.as_ref(),
                )
                .map_err(|e| format!("unable to decode log.data: {:?}", e))?;
            values.reverse();
            Ok(Self {
                sender: ethabi::decode(
                        &[ethabi::ParamType::Address],
                        log.topics[1usize].as_ref(),
                    )
                    .map_err(|e| {
                        format!(
                            "unable to decode param 'sender' from topic of type 'address': {:?}",
                            e
                        )
                    })?
                    .pop()
                    .expect(INTERNAL_ERR)
                    .into_address()
                    .expect(INTERNAL_ERR)
                    .as_bytes()
                    .to_vec(),
                recipient: ethabi::decode(
                        &[ethabi::ParamType::Address],
                        log.topics[2usize].as_ref(),
                    )
                    .map_err(|e| {
                        format!(
                            "unable to decode param 'recipient' from topic of type 'address': {:?}",
                            e
                        )
                    })?
                    .pop()
                    .expect(INTERNAL_ERR)
                    .into_address()
                    .expect(INTERNAL_ERR)
                    .as_bytes()
                    .to_vec(),
                amount0: {
                    let mut v = [0 as u8; 32];
                    values
                        .pop()
                        .expect(INTERNAL_ERR)
                        .into_int()
                        .expect(INTERNAL_ERR)
                        .to_big_endian(v.as_mut_slice());
                    substreams::scalar::BigInt::from_signed_bytes_be(&v)
                },
                amount1: {
                    let mut v = [0 as u8; 32];
                    values
                        .pop()
                        .expect(INTERNAL_ERR)
                        .into_int()
                        .expect(INTERNAL_ERR)
                        .to_big_endian(v.as_mut_slice());
                    substreams::scalar::BigInt::from_signed_bytes_be(&v)
                },
                sqrt_price_x96: {
                    let mut v = [0 as u8; 32];
                    values
                        .pop()
                        .expect(INTERNAL_ERR)
                        .into_uint()
                        .expect(INTERNAL_ERR)
                        .to_big_endian(v.as_mut_slice());
                    substreams::scalar::BigInt::from_unsigned_bytes_be(&v)
                },
                liquidity: {
                    let mut v = [0 as u8; 32];
                    values
                        .pop()
                        .expect(INTERNAL_ERR)
                        .into_uint()
                        .expect(INTERNAL_ERR)
                        .to_big_endian(v.as_mut_slice());
                    substreams::scalar::BigInt::from_unsigned_bytes_be(&v)
                },
                tick: {
                    let mut v = [0 as u8; 32];
                    values
                        .pop()
                        .expect(INTERNAL_ERR)
                        .into_int()
                        .expect(INTERNAL_ERR)
                        .to_big_endian(v.as_mut_slice());
                    substreams::scalar::BigInt::from_signed_bytes_be(&v)
                },
            })
        }
    }
    impl substreams_ethereum::Event for Swap {
        const NAME: &'static str = "Swap";
        fn match_log(log: &substreams_ethereum::pb::eth::v2::Log) -> bool {
            Self::match_log(log)
        }
        fn decode(log: &substreams_ethereum::pb::eth::v2::Log) -> Result<Self, String> {
            Self::decode(log)
        }
    }
    #[derive(Debug, Clone, PartialEq)]
    pub struct SwapFeeCharged {
        pub sender: Vec<u8>,
        pub fee0: substreams::scalar::BigInt,
        pub fee1: substreams::scalar::BigInt,
        pub fee_wad: substreams::scalar::BigInt,
    }
    impl SwapFeeCharged {
        const TOPIC_ID: [u8; 32] = [
            38u8,
            142u8,
            114u8,
            159u8,
            33u8,
            30u8,
            124u8,
            182u8,
            250u8,
            30u8,
            228u8,
            62u8,
            16u8,
            55u8,
            187u8,
            215u8,
            110u8,
            111u8,
            100u8,
            72u8,
            236u8,
            160u8,
            42u8,
            180u8,
            220u8,
            15u8,
            247u8,
            167u8,
            141u8,
            174u8,
            172u8,
            12u8,
        ];
        pub fn match_log(log: &substreams_ethereum::pb::eth::v2::Log) -> bool {
            if log.topics.len() != 2usize {
                return false;
            }
            if log.data.len() != 96usize {
                return false;
            }
            return log.topics.get(0).expect("bounds already checked").as_ref()
                == Self::TOPIC_ID;
        }
        pub fn decode(
            log: &substreams_ethereum::pb::eth::v2::Log,
        ) -> Result<Self, String> {
            let mut values = ethabi::decode(
                    &[
                        ethabi::ParamType::Uint(256usize),
                        ethabi::ParamType::Uint(256usize),
                        ethabi::ParamType::Uint(256usize),
                    ],
                    log.data.as_ref(),
                )
                .map_err(|e| format!("unable to decode log.data: {:?}", e))?;
            values.reverse();
            Ok(Self {
                sender: ethabi::decode(
                        &[ethabi::ParamType::Address],
                        log.topics[1usize].as_ref(),
                    )
                    .map_err(|e| {
                        format!(
                            "unable to decode param 'sender' from topic of type 'address': {:?}",
                            e
                        )
                    })?
                    .pop()
                    .expect(INTERNAL_ERR)
                    .into_address()
                    .expect(INTERNAL_ERR)
                    .as_bytes()
                    .to_vec(),
                fee0: {
                    let mut v = [0 as u8; 32];
                    values
                        .pop()
                        .expect(INTERNAL_ERR)
                        .into_uint()
                        .expect(INTERNAL_ERR)
                        .to_big_endian(v.as_mut_slice());
                    substreams::scalar::BigInt::from_unsigned_bytes_be(&v)
                },
                fee1: {
                    let mut v = [0 as u8; 32];
                    values
                        .pop()
                        .expect(INTERNAL_ERR)
                        .into_uint()
                        .expect(INTERNAL_ERR)
                        .to_big_endian(v.as_mut_slice());
                    substreams::scalar::BigInt::from_unsigned_bytes_be(&v)
                },
                fee_wad: {
                    let mut v = [0 as u8; 32];
                    values
                        .pop()
                        .expect(INTERNAL_ERR)
                        .into_uint()
                        .expect(INTERNAL_ERR)
                        .to_big_endian(v.as_mut_slice());
                    substreams::scalar::BigInt::from_unsigned_bytes_be(&v)
                },
            })
        }
    }
    impl substreams_ethereum::Event for SwapFeeCharged {
        const NAME: &'static str = "SwapFeeCharged";
        fn match_log(log: &substreams_ethereum::pb::eth::v2::Log) -> bool {
            Self::match_log(log)
        }
        fn decode(log: &substreams_ethereum::pb::eth::v2::Log) -> Result<Self, String> {
            Self::decode(log)
        }
    }
    #[derive(Debug, Clone, PartialEq)]
    pub struct Sync {
        pub reserve0: substreams::scalar::BigInt,
        pub reserve1: substreams::scalar::BigInt,
    }
    impl Sync {
        const TOPIC_ID: [u8; 32] = [
            207u8,
            42u8,
            165u8,
            8u8,
            118u8,
            205u8,
            251u8,
            181u8,
            65u8,
            32u8,
            111u8,
            137u8,
            175u8,
            14u8,
            231u8,
            141u8,
            68u8,
            162u8,
            171u8,
            248u8,
            211u8,
            40u8,
            227u8,
            127u8,
            164u8,
            145u8,
            127u8,
            152u8,
            33u8,
            73u8,
            132u8,
            138u8,
        ];
        pub fn match_log(log: &substreams_ethereum::pb::eth::v2::Log) -> bool {
            if log.topics.len() != 1usize {
                return false;
            }
            if log.data.len() != 64usize {
                return false;
            }
            return log.topics.get(0).expect("bounds already checked").as_ref()
                == Self::TOPIC_ID;
        }
        pub fn decode(
            log: &substreams_ethereum::pb::eth::v2::Log,
        ) -> Result<Self, String> {
            let mut values = ethabi::decode(
                    &[
                        ethabi::ParamType::Uint(256usize),
                        ethabi::ParamType::Uint(256usize),
                    ],
                    log.data.as_ref(),
                )
                .map_err(|e| format!("unable to decode log.data: {:?}", e))?;
            values.reverse();
            Ok(Self {
                reserve0: {
                    let mut v = [0 as u8; 32];
                    values
                        .pop()
                        .expect(INTERNAL_ERR)
                        .into_uint()
                        .expect(INTERNAL_ERR)
                        .to_big_endian(v.as_mut_slice());
                    substreams::scalar::BigInt::from_unsigned_bytes_be(&v)
                },
                reserve1: {
                    let mut v = [0 as u8; 32];
                    values
                        .pop()
                        .expect(INTERNAL_ERR)
                        .into_uint()
                        .expect(INTERNAL_ERR)
                        .to_big_endian(v.as_mut_slice());
                    substreams::scalar::BigInt::from_unsigned_bytes_be(&v)
                },
            })
        }
    }
    impl substreams_ethereum::Event for Sync {
        const NAME: &'static str = "Sync";
        fn match_log(log: &substreams_ethereum::pb::eth::v2::Log) -> bool {
            Self::match_log(log)
        }
        fn decode(log: &substreams_ethereum::pb::eth::v2::Log) -> Result<Self, String> {
            Self::decode(log)
        }
    }
}