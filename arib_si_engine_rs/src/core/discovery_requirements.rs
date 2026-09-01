#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeliverySystemRequirement {
    IsdbT,
    Bs,
    Cs110,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveryRequirement {
    pub delivery_system: DeliverySystemRequirement,
    pub require_pat: bool,
    pub require_pmt: bool,
    pub require_sdt_actual: bool,
    pub require_nit_actual: bool,
    pub require_bat: bool,
    pub require_sdt_other: bool,
    pub require_nit_other: bool,
}

pub fn requirement_for_original_network_id(original_network_id: u16) -> DiscoveryRequirement {
    match original_network_id {
        4 => satellite(DeliverySystemRequirement::Bs),
        6 | 7 => satellite(DeliverySystemRequirement::Cs110),
        _ => DiscoveryRequirement {
            delivery_system: DeliverySystemRequirement::IsdbT,
            require_pat: true,
            require_pmt: true,
            require_sdt_actual: true,
            require_nit_actual: true,
            require_bat: false,
            require_sdt_other: false,
            require_nit_other: false,
        },
    }
}

fn satellite(delivery_system: DeliverySystemRequirement) -> DiscoveryRequirement {
    DiscoveryRequirement {
        delivery_system,
        require_pat: true,
        require_pmt: true,
        require_sdt_actual: true,
        require_nit_actual: true,
        require_bat: true,
        require_sdt_other: true,
        require_nit_other: true,
    }
}

pub fn remote_key_fallback_service_id(service_id: u16) -> String {
    service_id.to_string()
}
