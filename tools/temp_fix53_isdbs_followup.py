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

replace_once(
    "tuner_hal2/device/src/px4/tune_mapping.rs",
    '''    #[test]
    fn isdbs_satellite_frequency_validation_normalizes_within_the_unambiguous_raster_cell() {
        assert_eq!(map_bs_if_frequency_to_px4_freq_no(1_049_480_000), Ok(0));
        assert_eq!(map_bs_if_frequency_to_px4_freq_no(1_050_000_000), Ok(0));
        assert_eq!(map_cs110_if_frequency_to_px4_freq_no(1_613_000_000), Ok(12));
        assert_eq!(map_cs110_if_frequency_to_px4_freq_no(1_613_499_999), Ok(12));
        assert!(map_bs_if_frequency_to_px4_freq_no(1_550_000_000).is_err());
        assert!(map_cs110_if_frequency_to_px4_freq_no(1_550_000_000).is_err());
    }
''',
    '''    #[test]
    fn isdbs_satellite_frequency_validation_uses_unambiguous_nearest_raster_center() {
        assert_eq!(map_bs_if_frequency_to_px4_freq_no(1_049_480_000), Ok(0));
        assert_eq!(map_bs_if_frequency_to_px4_freq_no(1_050_000_000), Ok(0));
        assert!(
            map_bs_if_frequency_to_px4_freq_no(PX4_BS_BASE_IF_HZ + PX4_BS_STEP_HZ / 2)
                .is_err()
        );
        assert_eq!(map_cs110_if_frequency_to_px4_freq_no(1_613_500_000), Ok(12));
        assert!(map_bs_if_frequency_to_px4_freq_no(1_550_000_000).is_err());
        assert!(map_cs110_if_frequency_to_px4_freq_no(1_550_000_000).is_err());
    }
''',
)

# The host-ci crates point their lib path outside each package directory. With
# Rust/Cargo 1.81, running all-target check before linked tests can reuse a stale
# dependency artifact for rustdoc. Tests first still cover doctests; the later
# all-target check preserves the same type-check coverage.
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
