"""Summarise a `lunaris mind --transcript` file: per turn the body's position, the nearest thing named in the goal, what the model said and did,
and what the server reported (tokens, cached tokens, milliseconds).

    python -I tools/mind_report.py target/renders/mind_real_1.json [--full N]     (--full N prints turn N completely)
"""
import json
import sys

sys.stdout.reconfigure(encoding="utf-8")
path = sys.argv[1]
doc = json.load(open(path, encoding="utf-8"))
full = int(sys.argv[sys.argv.index("--full") + 1]) if "--full" in sys.argv else None
print(f"model: {doc.get('model')}\ngoal: {doc.get('goal')}\nturns: {len(doc['turns'])}")
tot_prompt = tot_cached = tot_ms = 0
for i, t in enumerate(doc["turns"]):
    obs = t["observation"]
    pos = obs.get("you", {}).get("Transform", {}).get("pos")
    near = [(n.get("name"), n.get("distance")) for n in obs.get("nearby", [])[:2]]
    m = t.get("model", {})
    usage = m.get("usage", {})
    cached = usage.get("prompt_tokens_details", {}).get("cached_tokens", 0)
    tot_prompt += usage.get("prompt_tokens", 0)
    tot_cached += cached
    tot_ms += m.get("elapsed_ms", 0)
    acts = "; ".join(f"{a['tool']} {json.dumps(a['args'])[:70]}{'' if a['ok'] else ' FAILED ' + str(a['result'])[:60]}" for a in t["actions"])
    print(f"{i:>2} tick {t['tick']:>4} pos {pos and [round(x, 1) for x in pos]} near {near}\n     says: {t['thought'][:90]}\n     does: {acts}"
          f"\n     server: prompt {usage.get('prompt_tokens')} cached {cached} reply {usage.get('completion_tokens')} {m.get('elapsed_ms')} ms"
          + (f"\n     PROBLEM: {t['problem']}" if t.get("problem") else ""))
print(f"totals: prompt tokens {tot_prompt}, cached {tot_cached} ({100 * tot_cached / max(1, tot_prompt):.0f}%), model time {tot_ms / 1000:.1f} s")
if full is not None:
    print(json.dumps(doc["turns"][full], indent=1))
