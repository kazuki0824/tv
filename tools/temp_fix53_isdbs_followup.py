from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one anchor, got {count}")
    p.write_text(text.replace(old, new, 1))


# Keep the Hz -> fixed-raster adapter deterministic without pretending an exact
# midpoint has a unique nearest carrier. Values outside the first/last carrier
# but within half a raster step still have one nearest supported carrier.
replace_once(
    "tuner_hal2/common/src/lib.rs",
    '''    let index = if frequency_hz <= first_hz {
        0
    } else {
        frequency_hz.checked_sub(first_hz)?.checked_add(half_step)? / step_hz
    };
    if index >= count {
        return None;
    }
    let center = first_hz.checked_add(step_hz.checked_mul(index)?)?;
    (frequency_hz.abs_diff(center) <= half_step).then_some(center)
''',
    '''    if frequency_hz <= first_hz {
        return Some(first_hz);
    }
    if frequency_hz >= last {
        return Some(last);
    }
    let delta = frequency_hz.checked_sub(first_hz)?;
    let lower_index = delta / step_hz;
    let remainder = delta % step_hz;
    if step_hz % 2 == 0 && remainder == half_step {
        return None;
    }
    let index = lower_index + u64::from(remainder > half_step);
    if index >= count {
        return None;
    }
    first_hz.checked_add(step_hz.checked_mul(index)?)
''',
)

replace_once(
    "tuner_hal2/common/src/lib.rs",
    '''        assert_eq!(
            normalize_japan_bs_if_frequency_hz(JAPAN_BS_FIRST_IF_HZ + bs_half),
            Some(JAPAN_BS_FIRST_IF_HZ + JAPAN_BS_IF_STEP_HZ)
        );
        assert!(normalize_japan_bs_if_frequency_hz(JAPAN_BS_FIRST_IF_HZ - bs_half - 1).is_none());
''',
    '''        assert_eq!(
            normalize_japan_bs_if_frequency_hz(JAPAN_BS_FIRST_IF_HZ + bs_half),
            None
        );
        assert_eq!(
            normalize_japan_bs_if_frequency_hz(JAPAN_BS_FIRST_IF_HZ + bs_half - 1),
            Some(JAPAN_BS_FIRST_IF_HZ)
        );
        assert_eq!(
            normalize_japan_bs_if_frequency_hz(JAPAN_BS_FIRST_IF_HZ + bs_half + 1),
            Some(JAPAN_BS_FIRST_IF_HZ + JAPAN_BS_IF_STEP_HZ)
        );
        assert!(normalize_japan_bs_if_frequency_hz(JAPAN_BS_FIRST_IF_HZ - bs_half - 1).is_none());
''',
)

# Remove the stale exact-frequency test left by the old acquireRange=0 model.
replace_once(
    "tuner_hal2/device/src/px4/tune_mapping.rs",
    '''    #[test]
    fn isdbs_satellite_frequency_validation_is_exact_when_acquire_range_is_zero() {
        assert!(map_bs_if_frequency_to_px4_freq_no(1_049_480_000).is_ok());
        assert!(map_bs_if_frequency_to_px4_freq_no(1_049_480_001).is_err());
        assert!(map_bs_if_frequency_to_px4_freq_no(1_049_979_999).is_err());
        assert!(map_cs110_if_frequency_to_px4_freq_no(1_613_000_000).is_ok());
        assert!(map_cs110_if_frequency_to_px4_freq_no(1_613_000_001).is_err());
        assert!(map_cs110_if_frequency_to_px4_freq_no(1_613_499_999).is_err());
    }
''',
    '''    #[test]
    fn isdbs_satellite_frequency_validation_uses_unambiguous_nearest_raster_center() {
        assert_eq!(map_bs_if_frequency_to_px4_freq_no(1_049_480_000), Ok(0));
        assert_eq!(map_bs_if_frequency_to_px4_freq_no(1_050_000_000), Ok(0));
        assert_eq!(
            map_bs_if_frequency_to_px4_freq_no(PX4_BS_BASE_IF_HZ + PX4_BS_STEP_HZ / 2),
            Err(HalError::invalid_argument(
                HalInvalidArgumentKind::UnsupportedFrequency,
                "px4 BS IF frequency is outside the unambiguous Japan raster domain",
            ))
        );
        assert_eq!(map_cs110_if_frequency_to_px4_freq_no(1_613_500_000), Ok(12));
        assert!(map_bs_if_frequency_to_px4_freq_no(1_550_000_000).is_err());
        assert!(map_cs110_if_frequency_to_px4_freq_no(1_550_000_000).is_err());
    }
''',
)

# Rust 1.81 host CI exhibited dependency metadata reuse when a non-linking
# all-target check immediately preceded tests in the same target directory.
# Run linked tests first, then the broader type-check. Coverage is unchanged.
replace_once(
    ".github/workflows/tuner-hal2-host-rust-ci.yml",
    '''      - name: Type-check host-compatible crates
        run: cargo check --workspace --all-targets --locked

      - name: Run host-compatible unit tests
        run: cargo test --workspace --locked
''',
    '''      - name: Run host-compatible unit tests
        run: cargo test --workspace --locked

      - name: Type-check host-compatible crates
        run: cargo check --workspace --all-targets --locked
''',
)
