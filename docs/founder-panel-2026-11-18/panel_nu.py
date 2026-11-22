#!/usr/bin/env python3
"""NuViewer variant of founder-consumer's panel.py (the skill's own files are untouched).

Same dealing, same checks, same tally; adds: the app each buyer uses today (dealt from
customer.json "current_apps"), a try-vs-adopt funnel, a switch decision and the reasons to
switch / not switch. Usage identical:  python panel_nu.py --dir DIR init|prompts|check|save|tally|status
"""
import importlib.util
import json
import os
import random
import sys

SKILL = r"E:\founder-skill-main\skills\founder-consumer\panel.py"
spec = importlib.util.spec_from_file_location("panel_base", SKILL)
P = importlib.util.module_from_spec(spec)
spec.loader.exec_module(P)

SWITCH = ["replace", "use_alongside", "try_then_drop", "ignore"]
P.ANSWER_FIELDS = {"id", "buys", "reason_code", "reason", "would_try", "switch_decision",
                   "why_switch_or_not", "current_app_pros", "current_app_pains"}

_deal = P.deal


def deal(customer, n=100, seed=7):
    cards = _deal(customer, n, seed)
    apps = customer.get("current_apps") or []
    if apps:
        rng = random.Random(seed + 101)
        pool = []
        for a, k in zip(apps, P._quota(apps, n)):
            pool += [a] * k
        rng.shuffle(pool)
        for c, a in zip(cards, pool):
            c["current_app"] = a["name"]
            c["current_app_how"] = a.get("how", "")
    return cards


P.deal = deal

P.BRIEF = """You are a simulated PC user on a research panel. A developer wants to know, before spending more months on it, whether real people like you would use their app and why or why not. Your answer is only useful if it is honest: most people do not switch image viewers, and a polite yes is worse than a clear no.

=== WHO YOU ARE ===
{name}, {age}. {segment}. You earn about ${income:,} a year (local purchasing power, not important).
How you adopt software: {behaviour}. {behaviour_how}
The app you use today to look at pictures: {current_app}. {current_app_how}
{habit_line}The first doubt that comes to mind when you hear about a new viewer: "{objection}"
You are one of these people: {target}.
=== END ===

=== WHAT YOU HEAR ABOUT (you see it for the first time, e.g. a post or a download page) ===
{pitch}
=== END ===

Decide as {name} would, in a normal month. Be concrete about YOUR current app: what it does well for you and what bugs you (if nothing bugs you, say so). Do not be agreeable. Do not invent facts about NuViewer beyond what is written above, and do not assume it has features that are not listed. "buys" here means: you would make NuViewer your DEFAULT viewer for at least a month. "would_try" means you would install it and open some of your own files in it.

Create the file {out} with your file-writing tool (do not just print the answer in your reply) containing ONLY this JSON, no markdown fence, nothing else:
{{
  "id": "{id}",
  "would_try": true or false,
  "buys": true or false,
  "switch_decision": one of {switch},
  "purchases_first_month": on how many days in the first month you would open NuViewer (0 if never),
  "reason_code": one of {reasons} (the main reason for your adopt / not-adopt answer),
  "reason": "why, in one or two sentences, in your own voice",
  "why_switch_or_not": "the specific reasons you would switch to NuViewer, or would not switch, in your own words (2-3 sentences, name the features or doubts)",
  "current_app_pros": "what your current app does well for you",
  "current_app_pains": "what annoys you about your current app, or 'nothing'",
  "objection": "the doubt that weighs most for you, in your own words",
  "would_change_my_mind": "the one thing that would flip your answer, or empty",
  "price_too_cheap": 0,
  "price_cheap": 0,
  "price_expensive": a number: if NuViewer were paid, the one-time price in USD at which it would start to feel expensive but you might still buy (0 if you would never pay for a viewer),
  "price_too_expensive": a number: the one-time price in USD at which you would not buy it at all (0 if you would never pay)
}}
""".replace("{switch}", "{switch_}")

_orig_format_args = None


def write_briefs(d, wave=P.WAVE):
    state, cards = P.load_state(d)
    target = state["customer"]["target"]
    todo = []
    for c in cards:
        out = P._path(d, "answers", c["id"] + ".json")
        if P._answer_ok(out, c["id"]) is None:
            continue
        brief = P.BRIEF.format(pitch=state["pitch"], target=target, out=os.path.abspath(out),
                               habit_line=("Your day: %s.\n" % c["habit"]) if c.get("habit") else "",
                               reasons=" / ".join(P.REASONS), switch_=" / ".join(SWITCH),
                               **{k: v for k, v in c.items() if k not in ("habit",)})
        bp = P._path(d, "briefs", c["id"] + ".md")
        with open(bp, "w", encoding="utf-8") as fh:
            fh.write(brief)
        todo.append((c["id"], os.path.abspath(bp)))
    return [todo[i:i + wave] for i in range(0, len(todo), wave)]


P.write_briefs = write_briefs

_answer_ok = P._answer_ok


def answer_ok(path, pid):
    p = _answer_ok(path, pid)
    if p:
        return p
    a = P._read_json(path)
    if not isinstance(a.get("would_try"), bool):
        return "'would_try' must be true or false"
    if a.get("switch_decision") not in SWITCH:
        return "switch_decision %r not in %s" % (a.get("switch_decision"), SWITCH)
    return None


P._answer_ok = answer_ok

_tally = P.tally
_results_md = P.results_md


def tally(d):
    res = _tally(d)
    state, cards = P.load_state(d)
    by_id = {c["id"]: c for c in cards}
    rows = []
    for c in cards:
        path = P._path(d, "answers", c["id"] + ".json")
        if P._answer_ok(path, c["id"]):
            continue
        rows.append(dict(by_id[c["id"]], **P._read_json(path)))
    n = len(rows)

    def rate(rs, key):
        return sum(1 for r in rs if r[key]) / len(rs) if rs else 0.0

    def grp(key):
        o = {}
        for r in rows:
            o.setdefault(r[key], []).append(r)
        return sorted(((k, len(v), rate(v, "would_try"), rate(v, "buys")) for k, v in o.items()), key=lambda t: -t[3])

    res["try_rate"] = rate(rows, "would_try")
    res["tries"] = sum(1 for r in rows if r["would_try"])
    res["by_current_app"] = grp("current_app")
    res["by_segment2"] = grp("segment")
    res["by_behaviour2"] = grp("behaviour")
    sw = {}
    for r in rows:
        sw[r["switch_decision"]] = sw.get(r["switch_decision"], 0) + 1
    res["switch_decisions"] = sw
    days = [float(r.get("purchases_first_month") or 0) for r in rows if r["buys"]]
    res["days_open_if_adopt"] = sum(days) / len(days) if days else None
    pay = [float(r.get("price_expensive") or 0) for r in rows]
    res["would_pay_any"] = sum(1 for x in pay if x > 0)
    res["median_pay"] = sorted(pay)[n // 2] if pay else None
    return res


def results_md(res):
    pct = lambda v: "%.0f%%" % (v * 100)
    L = [_results_md(res).rstrip("\n"), "", "## Funnel", "",
         "- Would try (install + open own files): **%d / %d** (%s)" % (res["tries"], res["answered"], pct(res["try_rate"])),
         "- Would make it the default viewer: **%d / %d** (%s)" % (res["buys"], res["answered"], pct(res["buy_rate"])),
         "- Switch decisions: " + ", ".join("%s %d" % kv for kv in sorted(res["switch_decisions"].items(), key=lambda t: -t[1]))]
    L += ["", "## By current app", "", "| app today | users | try | default |", "| --- | ---: | ---: | ---: |"]
    for k, n, t, b in res["by_current_app"]:
        L.append("| %s | %d | %s | %s |" % (k, n, pct(t), pct(b)))
    return "\n".join(L) + "\n"


P.tally = tally
P.results_md = results_md

if __name__ == "__main__":
    sys.exit(P.main())
