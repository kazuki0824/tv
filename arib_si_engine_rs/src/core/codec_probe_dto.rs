use crate::codec_signaling::{AacAdtsConfiguration, AacConfigurationProbe};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum AacProbeStatusDto {
    #[serde(rename = "PENDING")]
    Pending,
    #[serde(rename = "INVALID")]
    Invalid,
    #[serde(rename = "READY")]
    Ready,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AacAdtsConfigurationDto {
    pub audio_object_type: i32,
    pub sampling_frequency: i32,
    pub extension_sampling_frequency: Option<i32>,
    pub channel_configuration: i32,
    pub channel_count: i32,
    pub audio_specific_config_hex: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AacConfigurationProbeDto {
    pub status: AacProbeStatusDto,
    pub reason: Option<String>,
    pub configuration: Option<AacAdtsConfigurationDto>,
}

impl From<AacAdtsConfiguration> for AacAdtsConfigurationDto {
    fn from(value: AacAdtsConfiguration) -> Self {
        Self {
            audio_object_type: i32::from(value.audio_object_type),
            sampling_frequency: value.sampling_frequency as i32,
            extension_sampling_frequency: value.extension_sampling_frequency.map(|item| item as i32),
            channel_configuration: i32::from(value.channel_configuration),
            channel_count: i32::from(value.channel_count),
            audio_specific_config_hex: value.audio_specific_config_hex,
        }
    }
}

impl From<AacConfigurationProbe> for AacConfigurationProbeDto {
    fn from(value: AacConfigurationProbe) -> Self {
        match value {
            AacConfigurationProbe::Pending => Self {
                status: AacProbeStatusDto::Pending,
                reason: None,
                configuration: None,
            },
            AacConfigurationProbe::Invalid { reason } => Self {
                status: AacProbeStatusDto::Invalid,
                reason: Some(reason.to_string()),
                configuration: None,
            },
            AacConfigurationProbe::Ready { configuration } => Self {
                status: AacProbeStatusDto::Ready,
                reason: None,
                configuration: Some(configuration.into()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_dto_preserves_probe_variants_without_json_contract() {
        assert_eq!(
            AacConfigurationProbeDto::from(AacConfigurationProbe::Pending),
            AacConfigurationProbeDto {
                status: AacProbeStatusDto::Pending,
                reason: None,
                configuration: None,
            }
        );

        let invalid = AacConfigurationProbeDto::from(AacConfigurationProbe::Invalid {
            reason: "invalid",
        });
        assert_eq!(invalid.status, AacProbeStatusDto::Invalid);
        assert_eq!(invalid.reason.as_deref(), Some("invalid"));
        assert!(invalid.configuration.is_none());

        let ready = AacConfigurationProbeDto::from(AacConfigurationProbe::Ready {
            configuration: AacAdtsConfiguration {
                audio_object_type: 2,
                sampling_frequency: 48_000,
                extension_sampling_frequency: None,
                channel_configuration: 2,
                channel_count: 2,
                audio_specific_config_hex: "1190".to_string(),
            },
        });
        assert_eq!(ready.status, AacProbeStatusDto::Ready);
        assert!(ready.reason.is_none());
        assert_eq!(ready.configuration.expect("configuration").channel_count, 2);
    }
}
