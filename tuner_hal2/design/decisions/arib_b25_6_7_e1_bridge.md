# STD-B25 6.7-E1 evidence bridge v62

## Primary source

- Authority: ARIB
- Standard: ARIB STD-B25 Version 6.7-E1, English translation
- Local file: `ARIB_STD-B25_v6_7-E1_EN.pdf`
- SHA-256: `b3d9794f4f1859eefd52c72970678acd6c497cbda44a5cbe7cd8dd9a56ed0d7b`
- Official source: https://www.arib.or.jp/english/html/overview/doc/6-STD-B25v6_7-E1.pdf

## Exact reviewed clauses for DP-162

- Part 1, 2.2.2.4: receiver descrambles TS packets using MULTI2.
- Part 1, 2.2.2.10: receiver receives ECM, transfers ECM to the IC card, and controls descrambling according to the IC-card response; Ks is protected by receiver/card authentication where applicable.
- Part 1, 2.2.2.11: receiver filters and transfers EMM payloads according to card/group IDs.
- Part 1, 3.1.5-3.1.7: scrambling applies at the TS layer, to the TS packet payload, per TS packet.
- Part 1, 3.2.3-3.2.4: ECM and EMM section/payload structures.
- Part 1, 4.3.3.3 Tables 4-11 to 4-14: ECM Receive transfers ECM and returns Ks/recording control; EMM Receive transfers EMM and returns status.
- Part 1, 4.8: transport_scrambling_control and adaptation_field_control identify scrambled/reserved packet states.
- Part 1, 4.9: at least one odd/even scrambling-key pair per tuner.
- Part 1, 4.10: at least 12 PIDs simultaneously.

## Design connection

STD-B25 defines receiver/CAS handling and the ECM/EMM/Ks data path inside the receiver and CA interface. It does not define a public Android Tuner HAL method that exposes ECM, EMM, Ks, or other key material to clients. Therefore:

1. DP-162's TS packet classification and scrambled/raw-record handling are compatible with STD-B25 6.7-E1.
2. The minimum simultaneous-capacity obligations in 4.9 and 4.10 are explicit product-capability inputs and must be separately advertised/enforced.
3. The rule that public Tuner HAL APIs do not expose ECM/EMM/Ks is an AOSP public-interface and least-exposure design conclusion, not a verbatim STD-B25 requirement.
4. Infrastructure framing corruption alone may quarantine the affected path; malformed TS/TEI/continuity handling remains scoped by the project failure taxonomy. STD-B25 supplies packet/CAS semantics, not Android quarantine/error-code policy.

## Closure

`B25_6_7_E1_FULL_TEXT_REVIEW = COMPLETE_FOR_DP_162`

Japanese Version 7.0 full-text equivalence is not claimed. The official 7.0 amendment summary must still be screened when making a Japanese-current certification claim, but that does not block the English-fallback design decision requested for DP-162.
