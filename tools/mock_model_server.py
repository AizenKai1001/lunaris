"""A stand-in for a language-model server, so `lunaris mind --endpoint` can be tried with no model and no network.

It speaks the OpenAI chat-completions protocol on localhost and "plays Orb Dash" with a few rules: it reads the observation JSON
in the last user message, finds the nearest orb and answers with `input` actions (WASD). A real model replaces this server
with no other change.

    python -I tools/mock_model_server.py --port 8099 &
    lunaris mind examples/orb_dash.lunaris.json --body 4 --goal "collect all 12 orbs" \\
        --endpoint http://127.0.0.1:8099/v1 --tools get,nearby,input --radius 80 --nearby-limit 60 --think-ticks 5 --turns 200
"""
import argparse
import json
from http.server import BaseHTTPRequestHandler, HTTPServer


def decide(observation: dict) -> dict:
    pos = observation["you"]["Transform"]["pos"]
    orbs = [e for e in observation.get("nearby", []) if str(e.get("name", "")).startswith("Orb ")]
    keys = {"w": False, "a": False, "s": False, "d": False}
    if not orbs:
        return {"thought": "nothing left to collect", "actions": [{"tool": "input", "args": {"values": keys}}]}
    target = min(orbs, key=lambda e: (e["pos"][0] - pos[0]) ** 2 + (e["pos"][2] - pos[2]) ** 2)
    dx, dz = target["pos"][0] - pos[0], target["pos"][2] - pos[2]
    keys.update({"d": dx > 0.2, "a": dx < -0.2, "s": dz > 0.2, "w": dz < -0.2})
    return {"thought": f"heading for {target['name']}", "actions": [{"tool": "input", "args": {"values": keys}}]}


class Handler(BaseHTTPRequestHandler):
    def do_POST(self):  # noqa: N802
        body = json.loads(self.rfile.read(int(self.headers.get("Content-Length", 0))))
        last = body["messages"][-1]["content"]
        observation = json.loads(last.split("Observation: ", 1)[1].split("\nReply with ONLY", 1)[0])
        reply = json.dumps(decide(observation))
        data = json.dumps({"choices": [{"message": {"role": "assistant", "content": reply}}]}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, *args):  # keep the console quiet
        pass


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=8099)
    args = ap.parse_args()
    print(f"mock model server on http://127.0.0.1:{args.port}/v1 (Ctrl+C to stop)", flush=True)
    HTTPServer(("127.0.0.1", args.port), Handler).serve_forever()
