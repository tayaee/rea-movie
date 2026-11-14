#!/usr/bin/env python3
"""
devtools/quota_checker.py
Queries quota usage for the 5 AI coding plans and provides:
  - display (pretty table matching '0' semantics)
  - JSON output
  - Best plan selection (highest remaining quota)
  - Round-robin plan selection (among available plans)
  - Single plan status check
"""

import argparse
import json
import subprocess
import sys
from pathlib import Path

# Ensure API keys come from the environment (never hardcode secrets here).
# OPENCODE_API_KEY is required for Plan 1; other plans read their own keys.

PLANS = {
    1: {
        "id": 1,
        "name": "opencode-muse",
        "tool": "opencode",
        "provider": "opencode.ai",
        "plan": "opencode-go",
        "model": "opencode-go/muse-spark-1.3-contributor",
        "script": "plan1-opencode.sh",
        "desc": "OpenCode Go (muse-spark-1.3-contributor)",
    },
    2: {
        "id": 2,
        "name": "pi-minimax",
        "tool": "pi",
        "provider": "minimax",
        "plan": "minimax-m3",
        "model": "minimax-m3",
        "script": "plan2-pi-minimax.sh",
        "desc": "MiniMax (minimax-m3 via pi)",
    },
    3: {
        "id": 3,
        "name": "agy-gemini",
        "tool": "agy",
        "provider": "antigravity",
        "plan": "gemini",
        "model": "gemini-3.8-flash-high",
        "script": "plan3-agy-gemini.sh",
        "desc": "Antigravity Gemini 3.8 Flash",
    },
    4: {
        "id": 4,
        "name": "agy-claude",
        "tool": "agy",
        "provider": "antigravity",
        "plan": "claude",
        "model": "claude-opus-5-5-high",
        "script": "plan4-agy-claude.sh",
        "desc": "Antigravity Claude Opus 5.5",
    },
    5: {
        "id": 5,
        "name": "agy-argon",
        "tool": "agy",
        "provider": "antigravity",
        "plan": "argon",
        "model": "gemini-4-argon-high",
        "script": "plan5-agy-argon.sh",
        "desc": "Antigravity Gemini 4 Argon",
    },
}

STATE_FILE = Path(__file__).resolve().parent / ".rr_state.json"


def run_cmd(cmd, timeout=12):
    try:
        res = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, check=False)
        return res.stdout.strip()
    except Exception:  # noqa: BLE001
        return ""


def get_plan1_quota():
    out = run_cmd(["opencode-go-usage.sh", "--json"])
    try:
        data = json.loads(out)
        usage = data.get("usage", data)
        rolling = usage.get("rolling", {})
        weekly = usage.get("weekly", {})
        roll_used = int(rolling.get("percent", 0))
        week_used = int(weekly.get("percent", 0))
        roll_ok = rolling.get("status") == "ok"
        week_ok = weekly.get("status") == "ok"
        is_ok = roll_ok and week_ok
        remaining_pct = max(0, 100 - max(roll_used, week_used)) if is_ok else 0
        return {
            "available": is_ok and remaining_pct > 0,
            "remaining_pct": remaining_pct,
            "status": "OK" if is_ok else "LIMIT",
            "detail": f"5h used: {roll_used}%, WK used: {week_used}%",
        }
    except Exception as e:  # noqa: BLE001
        return {"available": False, "remaining_pct": 0, "status": "ERROR", "detail": str(e)}


def get_plan2_quota():
    out = run_cmd(["minimax-usage.sh", "--json"])
    try:
        data = json.loads(out)
        remains = data.get("model_remains", [])
        general = next((m for m in remains if m.get("model_name") == "general"), remains[0] if remains else {})
        interval_rem = int(general.get("current_interval_remaining_percent", 0))
        weekly_rem = int(general.get("current_weekly_remaining_percent", 0))
        rem = min(interval_rem, weekly_rem)
        is_ok = rem > 0
        return {
            "available": is_ok,
            "remaining_pct": rem,
            "status": "OK" if is_ok else "LIMIT",
            "detail": f"5h rem: {interval_rem}%, WK rem: {weekly_rem}%",
        }
    except Exception as e:  # noqa: BLE001
        return {"available": False, "remaining_pct": 0, "status": "ERROR", "detail": str(e)}


def get_agy_quotas():
    out = run_cmd(["gemini-usage.sh", "--json"])
    results = {}
    try:
        data = json.loads(out)
        groups = data.get("command", {}).get("data", {}).get("groups", [])

        # Plan 3: Gemini Models
        gemini_grp = next((g for g in groups if "Gemini Models" in g.get("name", "")), {})
        g_buckets = {b.get("id"): b for b in gemini_grp.get("buckets", [])}
        g_5h = g_buckets.get("gemini-5h", {}).get("remaining_fraction", 0.0)
        g_wk = g_buckets.get("gemini-weekly", {}).get("remaining_fraction", 0.0)
        g_rem = int(min(g_5h, g_wk) * 100)
        results[3] = {
            "available": g_rem > 0,
            "remaining_pct": g_rem,
            "status": "OK" if g_rem > 0 else "LIMIT",
            "detail": f"5h rem: {int(g_5h*100)}%, WK rem: {int(g_wk*100)}%",
        }

        # Plan 4: Claude and GPT models
        claude_grp = next((g for g in groups if "Claude and GPT" in g.get("name", "")), {})
        c_buckets = {b.get("id"): b for b in claude_grp.get("buckets", [])}
        c_3p_wk = c_buckets.get("3p-weekly", {})
        c_3p_5h = c_buckets.get("3p-5h", {})
        c_wk_frac = c_3p_wk.get("remaining_fraction", 0.0)
        c_5h_frac = c_3p_5h.get("remaining_fraction", 0.0)
        c_disabled = c_3p_5h.get("disabled", False) or c_wk_frac <= 0.001
        c_rem = 0 if c_disabled else int(min(c_5h_frac, c_wk_frac) * 100)
        results[4] = {
            "available": (not c_disabled) and c_rem > 0,
            "remaining_pct": c_rem,
            "status": "EXHAUSTED" if c_disabled or c_rem <= 0 else "OK",
            "detail": f"WK rem: {int(c_wk_frac*100)}%, 5h rem: {int(c_5h_frac*100)}% (disabled={c_disabled})",
        }

        # Plan 5: Gemini 4 Argon
        argon_grp = next((g for g in groups if "Argon" in g.get("name", "")), {})
        a_buckets = {b.get("id"): b for b in argon_grp.get("buckets", [])}
        a_wk = next(iter(a_buckets.values()), {}).get("remaining_fraction", 0.0)
        a_rem = int(a_wk * 100)
        results[5] = {
            "available": a_rem > 0,
            "remaining_pct": a_rem,
            "status": "OK" if a_rem > 0 else "LIMIT",
            "detail": f"WK rem: {a_rem}%",
        }
    except Exception as e:  # noqa: BLE001
        err = {"available": False, "remaining_pct": 0, "status": "ERROR", "detail": str(e)}
        results[3] = results.get(3, err)
        results[4] = results.get(4, err)
        results[5] = results.get(5, err)
    return results


def fetch_all_quotas():
    quotas = {}
    quotas[1] = get_plan1_quota()
    quotas[2] = get_plan2_quota()
    agy = get_agy_quotas()
    quotas[3] = agy[3]
    quotas[4] = agy[4]
    quotas[5] = agy[5]

    for pid, plan in PLANS.items():
        plan["quota"] = quotas[pid]
    return PLANS


CODER_PLAN_IDS = (1, 2, 3, 4)


def get_best_plan(plans_data):
    # Sort available coder plans (1..4, excluding 5 Argon orchestrator) by remaining_pct descending
    coder_plans = [p for p in plans_data.values() if p["id"] in CODER_PLAN_IDS]
    available = [p for p in coder_plans if p["quota"]["available"]]
    if not available:
        # Fallback to any coder plan (1..4) with highest remaining
        available = coder_plans
    sorted_plans = sorted(available, key=lambda x: x["quota"]["remaining_pct"], reverse=True)
    return sorted_plans[0]["id"]


def get_next_rr_plan(plans_data):
    last_id = 0
    if STATE_FILE.exists():
        try:
            with open(STATE_FILE, "r") as f:
                state = json.load(f)
                last_id = state.get("last_plan_id", 0)
        except Exception:  # noqa: BLE001
            last_id = 0

    available_ids = [p["id"] for p in plans_data.values() if p["id"] in CODER_PLAN_IDS and p["quota"]["available"]]
    if not available_ids:
        available_ids = list(CODER_PLAN_IDS)

    # Find next cyclic ID among 1..4
    next_id = None
    for candidate in range(last_id + 1, 5):
        if candidate in available_ids:
            next_id = candidate
            break
    if next_id is None:
        for candidate in range(1, 5):
            if candidate in available_ids:
                next_id = candidate
                break

    # Save state
    try:
        with open(STATE_FILE, "w") as f:
            json.dump({"last_plan_id": next_id}, f)
    except Exception:  # noqa: BLE001, S110
        pass

    return next_id


def render_bar(pct, width=10):
    f = round(pct / 100 * width)
    return "█" * f + "░" * (width - f)


def display_table(plans_data):
    name_w = max(42, max(len(p["desc"]) for p in plans_data.values()) + 2)
    sep_len = name_w + 56
    print("=" * sep_len)
    print("  AI Coding Plans - Quota Status ('0' check)")
    print("=" * sep_len)
    best_id = get_best_plan(plans_data)
    for pid in range(1, 6):
        p = plans_data[pid]
        q = p["quota"]
        pct = q["remaining_pct"]
        bar = render_bar(pct)
        best_tag = " [★ BEST QUOTA]" if pid == best_id and q["available"] else ""
        status_color = "\033[32m" if q["available"] else "\033[31m"
        reset_color = "\033[0m"
        print(f" {pid}. {p['desc']:<{name_w}} | {bar} {pct:3d}% rem | {status_color}{q['status']:<9}{reset_color} | {q['detail']}{best_tag}")
    print("=" * sep_len)


def main():
    parser = argparse.ArgumentParser(description="Check quotas and select AI coder plan")
    parser.add_argument("--display", action="store_true", help="Display formatted table")
    parser.add_argument("--json", action="store_true", help="Output full JSON data")
    parser.add_argument("--best", action="store_true", help="Output best plan ID (most remaining quota)")
    parser.add_argument("--next-rr", action="store_true", help="Output next plan ID in round-robin")
    parser.add_argument("--check", type=int, help="Check if plan N is available (exit 0 if yes, 1 if no)")
    args = parser.parse_args()

    plans_data = fetch_all_quotas()

    if args.check:
        p = plans_data.get(args.check)
        if not p:
            sys.exit(2)
        if p["quota"]["available"]:
            sys.exit(0)
        else:
            sys.exit(1)

    if args.best:
        print(get_best_plan(plans_data))
        return

    if args.next_rr:
        print(get_next_rr_plan(plans_data))
        return

    if args.json:
        print(json.dumps(plans_data, indent=2))
        return

    # Default: display table
    display_table(plans_data)


if __name__ == "__main__":
    main()
