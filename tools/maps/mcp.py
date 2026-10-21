"""A small client for the editor's MCP server (docs/editor.md), for map scripts.

    import mcp
    c = mcp.Client()
    c.call("map_info", map="frozen-dream")

The token is the editor's per-install token in the game's user folder (CANOPY_MCP_TOKEN_FILE
overrides where it is read from); start the server with `just mcp` or from the editor."""
import json, os, base64, urllib.request

TOKEN_FILE = os.environ.get("CANOPY_MCP_TOKEN_FILE") or os.path.expanduser(
    "~/Library/Application Support/com.jt.canopy/editor-mcp-token")
URL = "http://127.0.0.1:7431/mcp"

class Client:
    def __init__(self):
        self.token = open(TOKEN_FILE).read().strip()
        self.session = None
        self.n = 0
        r = self._post({"jsonrpc": "2.0", "id": self._id(), "method": "initialize", "params": {
            "protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": "script", "version": "1"}}})
        self._post({"jsonrpc": "2.0", "method": "notifications/initialized"})

    def _id(self):
        self.n += 1
        return self.n

    def _post(self, body):
        req = urllib.request.Request(URL, data=json.dumps(body).encode(), method="POST")
        req.add_header("Content-Type", "application/json")
        req.add_header("Accept", "application/json, text/event-stream")
        req.add_header("Authorization", "Bearer " + self.token)
        if self.session:
            req.add_header("Mcp-Session-Id", self.session)
        with urllib.request.urlopen(req, timeout=120) as resp:
            if not self.session:
                self.session = resp.headers.get("Mcp-Session-Id")
            text = resp.read().decode()
        result = None
        for line in text.splitlines():
            if line.startswith("data:"):
                data = line[5:].strip()
                if data:
                    result = json.loads(data)
        if result is None and text.strip().startswith("{"):
            result = json.loads(text)
        return result

    def call(self, _name, **args):
        r = self._post({"jsonrpc": "2.0", "id": self._id(), "method": "tools/call",
                        "params": {"name": _name, "arguments": args}})
        if "error" in r:
            raise RuntimeError(r["error"])
        res = r["result"]
        block = res["content"][0]
        if res.get("isError"):
            raise RuntimeError(block.get("text"))
        if block["type"] == "image":
            return base64.b64decode(block["data"])
        try:
            return json.loads(block["text"])
        except Exception:
            return block["text"]

    def picture(self, map, path, scale=None):
        args = {"map": map}
        if scale:
            args["scale"] = scale
        open(path, "wb").write(self.call("map_picture", **args))
        return path
