from __future__ import annotations

import json
import re
import urllib.parse
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from functools import lru_cache
from html.parser import HTMLParser
from typing import Any

INDEX_URL = "https://ina4n.com/chideji/47info/chideji-index.html"
GSI_ADDRESS_SEARCH_URL = "https://msearch.gsi.go.jp/address-search/AddressSearch"
USER_AGENT = "maleicacid-tuner-hal2-vts-region-dataset/3"
CURRENT_ISDBT_FIRST_CHANNEL = 13
CURRENT_ISDBT_LAST_CHANNEL = 52
MAX_FETCH_WORKERS = 8
PREFECTURE_NAMES = (
    "北海道", "青森県", "岩手県", "宮城県", "秋田県", "山形県", "福島県",
    "茨城県", "栃木県", "群馬県", "埼玉県", "千葉県", "東京都", "神奈川県",
    "新潟県", "富山県", "石川県", "福井県", "山梨県", "長野県", "岐阜県",
    "静岡県", "愛知県", "三重県", "滋賀県", "京都府", "大阪府", "兵庫県",
    "奈良県", "和歌山県", "鳥取県", "島根県", "岡山県", "広島県", "山口県",
    "徳島県", "香川県", "愛媛県", "高知県", "福岡県", "佐賀県", "長崎県",
    "熊本県", "大分県", "宮崎県", "鹿児島県", "沖縄県",
)
LINK_NAME_TO_PREFECTURE = {
    name.removesuffix("都").removesuffix("府").removesuffix("県"): name
    for name in PREFECTURE_NAMES
}
LINK_NAME_TO_PREFECTURE["北海道"] = "北海道"
_DETAIL_PATH_RE = re.compile(r"/chideji/47tv/[^/]+/(?:\d{4}/)?[^/]+\.html$")


def _fetch_bytes(url: str) -> tuple[bytes, str | None]:
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return response.read(), response.headers.get_content_charset()
    except OSError as exc:
        raise RuntimeError(f"failed to fetch INA4N/GSI data from {url}: {exc}") from exc


def _fetch_text(url: str) -> str:
    raw, declared = _fetch_bytes(url)
    for encoding in (declared, "utf-8", "cp932", "shift_jis"):
        if not encoding:
            continue
        try:
            return raw.decode(encoding)
        except UnicodeDecodeError:
            continue
    raise RuntimeError(f"cannot decode {url}")


def _fetch_json_value(url: str) -> Any:
    raw, _ = _fetch_bytes(url)
    try:
        return json.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise RuntimeError(f"invalid JSON from {url}") from exc


class _LinkParser(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self._href: str | None = None
        self._text: list[str] = []
        self.links: list[tuple[str, str]] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        if tag != "a":
            return
        self._href = dict(attrs).get("href")
        self._text = []

    def handle_data(self, data: str) -> None:
        if self._href is not None:
            self._text.append(data)

    def handle_endtag(self, tag: str) -> None:
        if tag != "a" or self._href is None:
            return
        self.links.append(("".join(self._text).strip(), self._href))
        self._href = None
        self._text = []


class _PrefecturePageParser(HTMLParser):
    def __init__(self, page_url: str) -> None:
        super().__init__(convert_charrefs=True)
        self._page_url = page_url
        self._expect_coverage = False
        self._coverage_text = ""
        self._href: str | None = None
        self._anchor_text: list[str] = []
        self.transmitters: dict[str, dict[str, Any]] = {}

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        if tag == "a":
            self._href = dict(attrs).get("href")
            self._anchor_text = []

    def handle_data(self, data: str) -> None:
        text = " ".join(data.split())
        if not text:
            return
        if self._href is not None:
            self._anchor_text.append(text)
        if text == "主なカバーエリア":
            self._expect_coverage = True
            return
        if self._expect_coverage:
            self._coverage_text = text
            self._expect_coverage = False

    def handle_endtag(self, tag: str) -> None:
        if tag != "a" or self._href is None:
            return
        absolute = urllib.parse.urljoin(self._page_url, self._href)
        path = urllib.parse.urlparse(absolute).path
        if _DETAIL_PATH_RE.fullmatch(path) and not path.endswith("-index.html"):
            record = self.transmitters.setdefault(
                absolute,
                {"index_name": "".join(self._anchor_text).strip(), "coverage_texts": []},
            )
            if self._coverage_text and self._coverage_text not in record["coverage_texts"]:
                record["coverage_texts"].append(self._coverage_text)
        self._href = None
        self._anchor_text = []


class _DetailPageParser(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self._in_h1 = False
        self._h1: list[str] = []
        self._href: str | None = None
        self._anchor_text: list[str] = []
        self._in_row = False
        self._in_cell = False
        self._cell: list[str] = []
        self._row: list[str] = []
        self.rows: list[list[str]] = []
        self.map_links: list[tuple[str, str]] = []

    @property
    def title(self) -> str:
        return " ".join(self._h1).strip()

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        if tag == "h1":
            self._in_h1 = True
        elif tag == "a":
            self._href = dict(attrs).get("href")
            self._anchor_text = []
        elif tag == "tr":
            self._in_row = True
            self._row = []
        elif tag in {"td", "th"} and self._in_row:
            self._in_cell = True
            self._cell = []

    def handle_data(self, data: str) -> None:
        text = " ".join(data.split())
        if not text:
            return
        if self._in_h1:
            self._h1.append(text)
        if self._href is not None:
            self._anchor_text.append(text)
        if self._in_cell:
            self._cell.append(text)

    def handle_endtag(self, tag: str) -> None:
        if tag == "h1":
            self._in_h1 = False
        elif tag == "a" and self._href is not None:
            text = " ".join(self._anchor_text).strip()
            lowered = self._href.lower()
            if "yahoo" in lowered or "maps.google" in lowered or "google.com/maps" in lowered:
                self.map_links.append((text, self._href))
            self._href = None
            self._anchor_text = []
        elif tag in {"td", "th"} and self._in_cell:
            self._row.append(" ".join(self._cell).strip())
            self._cell = []
            self._in_cell = False
        elif tag == "tr" and self._in_row:
            if self._row:
                self.rows.append(self._row)
            self._row = []
            self._in_row = False


def _frequency_links(index_html: str) -> dict[str, str]:
    parser = _LinkParser()
    parser.feed(index_html)
    found: dict[str, str] = {}
    for text, href in parser.links:
        prefecture = LINK_NAME_TO_PREFECTURE.get(text)
        if prefecture is None:
            continue
        absolute = urllib.parse.urljoin(INDEX_URL, href)
        if "/47tv/" not in absolute or not absolute.endswith("-index.html"):
            continue
        found[prefecture] = absolute
    missing = sorted(set(PREFECTURE_NAMES) - set(found))
    if missing:
        raise RuntimeError("missing prefecture frequency links: " + ", ".join(missing))
    return found


@lru_cache(maxsize=1)
def frequency_links() -> dict[str, str]:
    return _frequency_links(_fetch_text(INDEX_URL))


def _area_keys(coverage: str) -> list[str]:
    normalized = coverage.replace("の一部", " ").replace("全域", " ")
    normalized = re.sub(r"[、，,・/／()（）]", " ", normalized)
    keys: set[str] = set()
    for city, ward in re.findall(
        r"([一-龯々ヶヵぁ-んァ-ヶー]{1,20}市)([一-龯々ヶヵぁ-んァ-ヶー]{1,20}区)",
        normalized,
    ):
        keys.add(city)
        keys.add(city + ward)
    for key in re.findall(r"[一-龯々ヶヵぁ-んァ-ヶー]{1,24}(?:市|区|町|村)", normalized):
        keys.add(key)
    return sorted(keys, key=lambda item: (-len(item), item))


def _parse_power_w(value: str) -> float:
    match = re.fullmatch(r"\s*([0-9]+(?:\.[0-9]+)?)\s*([kK]?[wW])\s*", value)
    if not match:
        raise RuntimeError(f"unsupported INA4N output value: {value!r}")
    amount = float(match.group(1))
    return amount * (1000.0 if match.group(2).lower() == "kw" else 1.0)


def _parse_coordinates(href: str) -> tuple[float, float] | None:
    parsed = urllib.parse.urlparse(href)
    query = urllib.parse.parse_qs(parsed.query)
    raw_lat = (query.get("lat") or query.get("hlat") or [None])[0]
    raw_lon = (query.get("lon") or query.get("hlon") or [None])[0]
    if raw_lat is not None and raw_lon is not None:
        try:
            latitude = float(raw_lat)
            longitude = float(raw_lon)
        except ValueError:
            return None
        if -90.0 <= latitude <= 90.0 and -180.0 <= longitude <= 180.0:
            return latitude, longitude
    match = re.search(r"/@(-?\d+(?:\.\d+)?),(-?\d+(?:\.\d+)?)", href)
    if match:
        latitude = float(match.group(1))
        longitude = float(match.group(2))
        if -90.0 <= latitude <= 90.0 and -180.0 <= longitude <= 180.0:
            return latitude, longitude
    return None


def _location_text(rows: list[list[str]]) -> str:
    for row in rows:
        if len(row) >= 2 and "中継局の場所" in row[0]:
            value = row[1].strip()
            if value:
                return value
    return ""


def _location_query(location_text: str) -> str:
    value = re.sub(r"\s+", " ", location_text).strip()
    starts = [value.find(prefecture) for prefecture in PREFECTURE_NAMES if value.find(prefecture) >= 0]
    if starts:
        value = value[min(starts):]
    value = re.split(
        r"(?:及び|および|\s+(?:NHK|HBC|STV|HTB|TVh|UHB|民放)[^：:]{0,30}[：:])",
        value,
        maxsplit=1,
    )[0]
    return re.sub(r"[（(].*$", "", value).strip()


def _geocode_location(location_text: str) -> tuple[float, float] | None:
    query = _location_query(location_text)
    if not query:
        return None
    url = GSI_ADDRESS_SEARCH_URL + "?" + urllib.parse.urlencode({"q": query})
    try:
        value = _fetch_json_value(url)
    except RuntimeError:
        return None
    if not isinstance(value, list) or not value:
        return None
    feature = value[0]
    if not isinstance(feature, dict):
        return None
    geometry = feature.get("geometry")
    if not isinstance(geometry, dict):
        return None
    coordinates = geometry.get("coordinates")
    if not isinstance(coordinates, list) or len(coordinates) < 2:
        return None
    try:
        longitude = float(coordinates[0])
        latitude = float(coordinates[1])
    except (TypeError, ValueError):
        return None
    if not -90.0 <= latitude <= 90.0 or not -180.0 <= longitude <= 180.0:
        return None
    return latitude, longitude


def _services(rows: list[list[str]]) -> list[dict[str, Any]]:
    result: list[dict[str, Any]] = []
    for row in rows:
        if len(row) < 5:
            continue
        remote = row[1].strip()
        physical = row[2].strip()
        if not remote.isdigit() or not physical.isdigit():
            continue
        remote_id = int(remote)
        channel = int(physical)
        if not 1 <= remote_id <= 12 or not CURRENT_ISDBT_FIRST_CHANNEL <= channel <= CURRENT_ISDBT_LAST_CHANNEL:
            continue
        output_text = row[4].strip()
        try:
            output_w = _parse_power_w(output_text) if output_text else None
        except RuntimeError:
            output_w = None
        result.append(
            {
                "name": row[0].strip(),
                "remote_control_key_id": remote_id,
                "physical_channel": channel,
                "polarization": row[3].strip() or None,
                "output_text": output_text,
                "output_w": output_w,
            }
        )
    result.sort(key=lambda item: (item["remote_control_key_id"], item["physical_channel"], item["name"]))
    return result


def _transmitter_id(url: str) -> str:
    path = urllib.parse.urlparse(url).path
    marker = "/chideji/47tv/"
    if marker not in path:
        raise RuntimeError(f"unexpected INA4N transmitter URL: {url}")
    return path.split(marker, 1)[1]


def _coordinate_override(
    transmitter_id: str,
    overrides: dict[str, Any] | None,
) -> tuple[tuple[float, float], str] | None:
    if not overrides:
        return None
    raw = overrides.get(transmitter_id)
    if not isinstance(raw, dict):
        return None
    if raw.get("source") != "A-PAB":
        raise RuntimeError(f"coordinate override for {transmitter_id} must declare source=A-PAB")
    try:
        latitude = float(raw["latitude"])
        longitude = float(raw["longitude"])
    except (KeyError, TypeError, ValueError) as exc:
        raise RuntimeError(f"invalid coordinate override for {transmitter_id}") from exc
    if not -90.0 <= latitude <= 90.0 or not -180.0 <= longitude <= 180.0:
        raise RuntimeError(f"coordinate override for {transmitter_id} is out of range")
    return (latitude, longitude), "A-PAB"


def _build_transmitter(
    prefecture: str,
    detail_url: str,
    index_record: dict[str, Any],
    detail_html: str,
    coordinate_overrides: dict[str, Any] | None = None,
) -> dict[str, Any] | None:
    parser = _DetailPageParser()
    parser.feed(detail_html)
    services = _services(parser.rows)
    if not services:
        return None
    location_text = _location_text(parser.rows)
    transmitter_id = _transmitter_id(detail_url)
    coordinates: tuple[float, float] | None = None
    coordinate_source: str | None = None
    if parser.map_links:
        map_text, map_href = parser.map_links[0]
        coordinates = _parse_coordinates(map_href)
        if coordinates is not None:
            coordinate_source = "INA4N-map"
        if map_text:
            location_text = map_text
    if coordinates is None:
        override = _coordinate_override(transmitter_id, coordinate_overrides)
        if override is not None:
            coordinates, coordinate_source = override
    if coordinates is None and location_text:
        coordinates = _geocode_location(location_text)
        if coordinates is not None:
            coordinate_source = "GSI-from-INA4N-location"
    latitude = coordinates[0] if coordinates is not None else None
    longitude = coordinates[1] if coordinates is not None else None
    coverage_texts = sorted(
        {str(item).strip() for item in index_record.get("coverage_texts", []) if str(item).strip()}
    )
    coverage_areas = sorted({key for text in coverage_texts for key in _area_keys(text)})
    name = parser.title or str(index_record.get("index_name") or transmitter_id)
    return {
        "id": transmitter_id,
        "prefecture": prefecture,
        "name": name,
        "source_url": detail_url,
        "location_text": location_text,
        "latitude": latitude,
        "longitude": longitude,
        "coordinate_source": coordinate_source,
        "coverage_texts": coverage_texts,
        "coverage_areas": coverage_areas,
        "services": services,
    }


def _prefecture_transmitter_refs(
    prefecture: str,
    page_url: str,
    html: str,
) -> list[tuple[str, str, dict[str, Any]]]:
    parser = _PrefecturePageParser(page_url)
    parser.feed(html)
    if not parser.transmitters:
        raise RuntimeError(f"no INA4N transmitters found for {prefecture}: {page_url}")
    return [(prefecture, url, record) for url, record in sorted(parser.transmitters.items())]


@lru_cache(maxsize=47)
def load_prefecture(prefecture: str) -> tuple[dict[str, Any], ...]:
    return tuple(load_prefecture_with_overrides(prefecture, None))


def load_prefecture_with_overrides(
    prefecture: str,
    coordinate_overrides: dict[str, Any] | None,
) -> list[dict[str, Any]]:
    if prefecture not in PREFECTURE_NAMES:
        raise RuntimeError(f"unsupported Japanese prefecture: {prefecture}")
    page_url = frequency_links()[prefecture]
    refs = _prefecture_transmitter_refs(prefecture, page_url, _fetch_text(page_url))

    def fetch_one(ref: tuple[str, str, dict[str, Any]]) -> dict[str, Any] | None:
        item_prefecture, detail_url, index_record = ref
        return _build_transmitter(
            item_prefecture,
            detail_url,
            index_record,
            _fetch_text(detail_url),
            coordinate_overrides,
        )

    with ThreadPoolExecutor(max_workers=MAX_FETCH_WORKERS) as executor:
        raw = list(executor.map(fetch_one, refs))
    transmitters = [item for item in raw if item is not None]
    transmitters.sort(key=lambda item: item["id"])
    if not transmitters:
        raise RuntimeError(f"INA4N has no current ISDB-T transmitter data for {prefecture}")
    return transmitters


@lru_cache(maxsize=1)
def load_all() -> tuple[dict[str, Any], ...]:
    return tuple(load_all_with_overrides(None))


def load_all_with_overrides(
    coordinate_overrides: dict[str, Any] | None,
) -> list[dict[str, Any]]:
    links = frequency_links()

    def fetch_prefecture(prefecture: str) -> list[tuple[str, str, dict[str, Any]]]:
        page_url = links[prefecture]
        return _prefecture_transmitter_refs(
            prefecture,
            page_url,
            _fetch_text(page_url),
        )

    with ThreadPoolExecutor(max_workers=MAX_FETCH_WORKERS) as executor:
        grouped = list(executor.map(fetch_prefecture, PREFECTURE_NAMES))
    refs = [ref for group in grouped for ref in group]

    def fetch_one(ref: tuple[str, str, dict[str, Any]]) -> dict[str, Any] | None:
        prefecture, detail_url, index_record = ref
        return _build_transmitter(
            prefecture,
            detail_url,
            index_record,
            _fetch_text(detail_url),
            coordinate_overrides,
        )

    with ThreadPoolExecutor(max_workers=MAX_FETCH_WORKERS) as executor:
        raw = list(executor.map(fetch_one, refs))
    transmitters = [item for item in raw if item is not None]
    transmitters.sort(key=lambda item: item["id"])
    if not transmitters:
        raise RuntimeError("INA4N has no current nationwide ISDB-T transmitter data")
    return transmitters

def live_descriptor() -> dict[str, Any]:
    return {
        "schema_version": 3,
        "mode": "live-ina4n",
        "source": {
            "index_url": INDEX_URL,
            "source_notice": (
                "ISDB-T transmitter/channel/output/polarization/coverage facts are read from INA4N; "
                "A-PAB may be used only as an explicit coordinate override when INA4N has no map "
                "coordinate; GSI may geocode INA4N location text when no override is supplied"
            ),
        },
        "coordinate_overrides": {},
    }


def generate_snapshot(coordinate_overrides: dict[str, Any] | None = None) -> dict[str, Any]:
    transmitters: list[dict[str, Any]] = []
    for prefecture in PREFECTURE_NAMES:
        transmitters.extend(load_prefecture_with_overrides(prefecture, coordinate_overrides))
    transmitters.sort(key=lambda item: item["id"])
    return {
        "schema_version": 3,
        "mode": "snapshot",
        "source": live_descriptor()["source"],
        "transmitters": transmitters,
    }
