#!/usr/bin/env python3
"""Build the STS2 English -> Simplified Chinese terminology snapshot.

Spire Codex extracts these display names from the game's own localization files.
The generated snapshot is consumed locally by the plugin, so the running bot does
not depend on the third-party API.
"""

from __future__ import annotations

import argparse
import json
import sys
import urllib.parse
import urllib.request
from collections import Counter, defaultdict
from datetime import UTC, datetime
from pathlib import Path


ENTITY_ENDPOINTS = (
    "cards",
    "characters",
    "relics",
    "monsters",
    "potions",
    "enchantments",
    "encounters",
    "events",
    "powers",
    "keywords",
    "intents",
    "orbs",
    "afflictions",
    "modifiers",
    "achievements",
    "badges",
    "epochs",
    "stories",
    "acts",
    "ascensions",
)


def fetch_json(api_base: str, endpoint: str, language: str, channel: str):
    query = urllib.parse.urlencode({"lang": language, "channel": channel})
    url = f"{api_base.rstrip('/')}/{endpoint}?{query}"
    request = urllib.request.Request(url, headers={"User-Agent": "kovi-sts2-terms/1.0"})
    with urllib.request.urlopen(request, timeout=60) as response:
        return json.load(response)


def localized_pair(english: object, chinese: object) -> tuple[str, str] | None:
    if not isinstance(english, str) or not isinstance(chinese, str):
        return None
    english = english.strip()
    chinese = chinese.strip()
    if not english or english == chinese or not any("\u3400" <= char <= "\u9fff" for char in chinese):
        return None
    return english, chinese


def paired_items(english_items: list[dict], chinese_items: list[dict]):
    chinese_by_id = {str(item.get("id")): item for item in chinese_items if "id" in item}
    for english_item in english_items:
        entity_id = str(english_item.get("id"))
        chinese_item = chinese_by_id.get(entity_id)
        if chinese_item is not None:
            yield english_item, chinese_item


def add_pair(candidates: dict[str, list[tuple[str, str]]], pair, source: str) -> None:
    if pair is not None:
        english, chinese = pair
        candidates[english].append((chinese, source))


def collect_entity_terms(api_base: str, channel: str, candidates) -> None:
    for endpoint in ENTITY_ENDPOINTS:
        english_items = fetch_json(api_base, f"api/{endpoint}", "eng", channel)
        chinese_items = fetch_json(api_base, f"api/{endpoint}", "zhs", channel)
        if not isinstance(english_items, list) or not isinstance(chinese_items, list):
            raise ValueError(f"api/{endpoint} did not return a list")

        for english_item, chinese_item in paired_items(english_items, chinese_items):
            add_pair(
                candidates,
                localized_pair(
                    english_item.get("name", english_item.get("title")),
                    chinese_item.get("name", chinese_item.get("title")),
                ),
                endpoint,
            )

            if endpoint == "monsters":
                for english_move, chinese_move in paired_items(
                    english_item.get("moves") or [], chinese_item.get("moves") or []
                ):
                    add_pair(
                        candidates,
                        localized_pair(english_move.get("name"), chinese_move.get("name")),
                        "monster_moves",
                    )


def flatten_mapping(value, path=()):
    if isinstance(value, dict):
        for key, child in value.items():
            yield from flatten_mapping(child, (*path, str(key)))
    elif isinstance(value, str):
        yield path, value


def collect_shared_terms(api_base: str, channel: str, candidates) -> None:
    english = dict(flatten_mapping(fetch_json(api_base, "api/translations", "eng", channel)))
    chinese = dict(flatten_mapping(fetch_json(api_base, "api/translations", "zhs", channel)))
    for path, english_text in english.items():
        add_pair(
            candidates,
            localized_pair(english_text, chinese.get(path)),
            f"translations/{'/'.join(path[:-1])}",
        )


def choose_terms(candidates):
    terms = []
    for english, choices in candidates.items():
        counts = Counter(chinese for chinese, _ in choices)
        variants = []
        for chinese, count in counts.most_common():
            variant = {"chinese": chinese}
            if len(counts) > 1:
                variant["sources"] = sorted(
                    {source for value, source in choices if value == chinese}
                )
            variants.append(variant)
        if len(counts) > 1:
            summary = ", ".join(f"{name} ({count})" for name, count in counts.most_common())
            print(f"warning: {english!r} has variants: {summary}", file=sys.stderr)
        terms.append({"english": english, "variants": variants})
    return sorted(
        terms,
        key=lambda item: (-len(item["english"]), item["english"].casefold()),
    )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--api-base", default="https://spire-codex.com")
    parser.add_argument("--channel", default="beta")
    parser.add_argument(
        "--output",
        type=Path,
        default=Path(__file__).resolve().parents[1] / "assets" / "official_terms.zhs.json",
    )
    args = parser.parse_args()

    candidates = defaultdict(list)
    collect_entity_terms(args.api_base, args.channel, candidates)
    collect_shared_terms(args.api_base, args.channel, candidates)
    terms = choose_terms(candidates)

    document = {
        "schema_version": 1,
        "generated_at": datetime.now(UTC).replace(microsecond=0).isoformat(),
        "source": f"{args.api_base.rstrip('/')}/api",
        "source_note": "Names extracted by Spire Codex from Slay the Spire 2 localization files.",
        "channel": args.channel,
        "terms": terms,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        json.dumps(document, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    print(f"wrote {len(terms)} terms to {args.output}")


if __name__ == "__main__":
    main()
