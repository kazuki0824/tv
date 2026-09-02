from __future__ import annotations

import argparse
import json
import re
import urllib.parse
import urllib.request
from html.parser import HTMLParser
from pathlib import Path

INDEX_URL = "https://ina4n.com/chideji/47info/chideji-index.html"
USER_AGENT = "maleicacid-tuner-hal2-vts-region-dataset/1"
CURRENT_ISDBT_FIRST_CHANNEL = 13
CURRENT_ISDBT_LAST_CHANNEL = 52
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


def _fetch_text(url: str) -> str:
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(request, timeout=30) as response:
        raw = response.read()
        declared = response.headers.get_content_charset()
    candidates = [declared, "utf-8", "cp932", "shift_jis"]
    for encoding in candidates:
        if not encoding:
            continue
        try:
            return raw.decode(encoding)
        except UnicodeDecodeError:
            continue
    raise RuntimeError(f"cannot decode {url}")


class _IndexParser(HTMLParser):
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
        text = "".join(self._text).strip()
        self.links.append((text, self._href))
        self._href = None
        self._text = []


class _PrefectureParser(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self._expect_coverage = False
        self._current_coverage = ""
        self._in_cell = False
        self._cell: list[str] = []
        self._row: list[str] = []
        self._in_row = False
        self.coverage_channels: dict[str, set[int]] = {}
        self.all_channels: set[int] = set()
        self.default_channels: set[int] | None = None

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        del attrs
        if tag == "tr":
            self._in_row = True
            self._row = []
        elif tag in {"td", "th"} and self._in_row:
            self._in_cell = True
            self._cell = []

    def handle_data(self, data: str) -> None:
        text = " ".join(data.split())
        if not text:
            return
        if self._in_cell:
            self._cell.append(text)
        if text == "主なカバーエリア":
            self._expect_coverage = True
            return
        if self._expect_coverage:
            self._current_coverage = text
            self._expect_coverage = False

    def handle_endtag(self, tag: str) -> None:
        if tag in {"td", "th"} and self._in_cell:
            self._row.append(" ".join(self._cell).strip())
            self._cell = []
            self._in_cell = False
            return
        if tag != "tr" or not self._in_row:
            return
        self._consume_row(self._row)
        self._row = []
        self._in_row = False

    def _consume_row(self, cells: list[str]) -> None:
        numeric: list[int] = []
        saw_small_number = False
        for cell in cells:
            value = cell.strip()
            if not re.fullmatch(r"\d+", value):
                continue
            number = int(value)
            if 1 <= number <= 12:
                saw_small_number = True
            elif CURRENT_ISDBT_FIRST_CHANNEL <= number <= CURRENT_ISDBT_LAST_CHANNEL:
                numeric.append(number)
        if not numeric or saw_small_number:
            return
        channels = set(numeric)
        self.all_channels.update(channels)
        if self.default_channels is None:
            self.default_channels = set(channels)
        if self._current_coverage:
            self.coverage_channels.setdefault(self._current_coverage, set()).update(channels)


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


def _frequency_links(index_html: str) -> dict[str, str]:
    parser = _IndexParser()
    parser.feed(index_html)
    found: dict[str, str] = {}
    for text, href in parser.links:
        prefecture = LINK_NAME_TO_PREFECTURE.get(text)
        if prefecture is None:
            continue
        absolute = urllib.parse.urljoin(INDEX_URL, href)
        if "/47tv/" not in absolute:
            continue
        found[prefecture] = absolute
    missing = sorted(set(PREFECTURE_NAMES) - set(found))
    if missing:
        raise RuntimeError("missing prefecture frequency links: " + ", ".join(missing))
    return found


def _build_prefecture(url: str) -> dict[str, object]:
    parser = _PrefectureParser()
    parser.feed(_fetch_text(url))
    if not parser.all_channels:
        raise RuntimeError(f"no current physical channels parsed from {url}")
    areas: dict[str, set[int]] = {}
    for coverage, channels in parser.coverage_channels.items():
        for key in _area_keys(coverage):
            areas.setdefault(key, set()).update(channels)
    return {
        "source_url": url,
        "default_channels": sorted(parser.default_channels or parser.all_channels),
        "prefecture_channels": sorted(parser.all_channels),
        "areas": {key: sorted(channels) for key, channels in sorted(areas.items())},
    }


def generate() -> dict[str, object]:
    links = _frequency_links(_fetch_text(INDEX_URL))
    prefectures = {
        prefecture: _build_prefecture(links[prefecture])
        for prefecture in PREFECTURE_NAMES
    }
    if len(prefectures) != 47:
        raise RuntimeError(f"expected 47 prefectures, got {len(prefectures)}")
    return {
        "schema_version": 2,
        "source": {
            "index_url": INDEX_URL,
            "source_notice": (
                "community-maintained nationwide terrestrial relay listing; "
                "snapshot normalized to current Japan ISDB-T physical channels 13-52"
            ),
        },
        "prefectures": prefectures,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    dataset = generate()
    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(
        json.dumps(dataset, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
