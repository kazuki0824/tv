#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DiscoveryProfile {
    #[default]
    IsdbT,
    Bs,
    Cs110,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OptionalTableRequirement {
    pub require_sdt_other: bool,
    pub require_nit_other: bool,
}

pub fn optional_table_requirement(profile: DiscoveryProfile) -> OptionalTableRequirement {
    match profile {
        DiscoveryProfile::IsdbT => OptionalTableRequirement {
            require_sdt_other: false,
            require_nit_other: false,
        },
        DiscoveryProfile::Bs => OptionalTableRequirement {
            require_sdt_other: true,
            require_nit_other: false,
        },
        DiscoveryProfile::Cs110 => OptionalTableRequirement {
            require_sdt_other: true,
            require_nit_other: true,
        },
    }
}
