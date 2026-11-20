# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""tools/http-coverage.py on a small made-up tree: which routes and tools it reads, and what it finds missing."""
import contextlib
import importlib.util
import io
import shutil
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("http_coverage", ROOT / "tools/http-coverage.py")
cov = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cov)

HTTP_RS = r'''impl Http {
    fn answer(&mut self, s: &TcpStream, ui: &mut Ui) -> bool {
        let peer_ok = true; // the peer's address
        let (ctype, body): (&str, Vec<u8>) = match route {
            _ if foreign(&text, route, post) => ("text/plain", b"refused".to_vec()),
            "/update" | "/mcp" if (post || route == "/mcp") && !peer_ok => ("text/plain", b"refused".to_vec()),
            "/" => ("text/html", PAGE.as_bytes().to_vec()),
            // the scope's state: a comment with an apostrophe and a brace { that count for nothing
            "/ui-state" if !post => ("application/json", format!("{{\"a\":{}}}", 1).into_bytes()),
            "/waveform" if post => {
                let slot = query.split('&').find(|p| p.starts_with("slot=")); // '}' in a comment
                match slot {
                    Some(_) => return false,
                    None => ("text/plain", b"=> no slot".to_vec()),
                }
            }
            "/screen.png" | "/screen.jpg" => {
                self.shots.request(s);
                return false;
            }
            "/wave.csv" => match ui.web_wave_csv() {
                Ok(t) => ("text/csv", t.into_bytes()),
                Err(e) => ("text/plain", e.into_bytes()),
            },
            "/update" if post => ("text/plain", b"ok".to_vec()),
            "/update" => ("text/html", r#"<a href="/x">{</a>"#.as_bytes().to_vec()),
            "/mcp" => match mcp(body, ui) {
                Some(r) => ("application/json", r.into_bytes()),
                None => ("", Vec::new()),
            },
            _ => ("text/plain", b"not found".to_vec()),
        };
        true
    }
}

fn methods_of(route: &str) -> Option<&'static str> {
    match route {
        "/waveform" | "/mcp" => Some("POST"),
        "/update" => Some("GET, HEAD, POST"),
        "/screen.png" | "/screen.jpg" => Some("GET"),
        "/" | "/ui-state" | "/wave.csv" => Some("GET, HEAD"),
        _ => None,
    }
}

pub fn mcp(body: &str, ui: &mut Ui) -> Option<String> {
    let result = match method {
        "initialize" => J::obj(&[("serverInfo", J::obj(&[("name", J::s("oos-ui"))]))]),
        "tools/list" => J::obj(&[(
            "tools",
            J::Arr(vec![
                J::obj(&[("name", J::s("scpi")), ("description", J::s("Run SCPI."))]),
                J::obj(&[("name", J::s("screenshot")), ("description", J::s("The screen."))]),
            ]),
        )]),
        "tools/call" => match params.get("name").and_then(J::str) {
            Some("scpi") => text("ok"),
            Some("screenshot") => text("png"),
            _ => return Some(err(id, -32602.0, "unknown tool")),
        },
        _ => return Some(err(id, -32601.0, "method not found")),
    };
    Some(result.to_string())
}
'''

HTTP_MD = """# Reference

## Common answers

### Not an entry

- Mark: **stable**

## Routes

### `GET /`

- Mark: **internal**

### `GET /ui-state`

- Mark: **experimental** (may change)

### `POST /waveform`

- Mark: **stable**

### `GET /screen.png`

- Mark: **stable**

### `GET /screen.jpg`

- Mark: **stable**

### `GET /wave.csv`

- Mark: **stable**

### `GET /update`

- Mark: **internal**

### `POST /update`

- Mark: **stable**

### `POST /mcp`

- Mark: **stable**

### MCP tool `scpi`

- Mark: **stable**

### MCP tool `screenshot`

- Mark: **stable**

```sh
# a comment in a block of code is no heading
```
"""


class Coverage(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp(prefix="oos-http-coverage-"))
        self.addCleanup(shutil.rmtree, self.root)
        self.write(HTTP_RS, HTTP_MD)

    def write(self, rs, md, site="// the screen's routes: none in these tests but the one that says so\n"):
        for rel, text in ((cov.HTTP_RS, rs), (cov.SITE_RS, site), (cov.HTTP_MD, md)):
            (self.root / rel).parent.mkdir(parents=True, exist_ok=True)
            (self.root / rel).write_text(text)

    def findings(self):
        return cov.check(str(self.root))[0]

    def test_a_route_the_screen_answers_counts_after_those_of_the_transport(self):
        site = 'pub fn answer() { match route { "/version" => ok(), "/update" => ok(), _ => None } }\n'
        self.write(HTTP_RS, HTTP_MD, site)
        found = self.findings()
        self.assertIn("ui/remote/src/http.rs: route GET/POST /version has no entry `### \\`GET /version\\`` in docs/http.md",
                      found)
        self.assertIn("ui/remote/src/http.rs: route /version has no arm in fn methods_of: it would take every method",
                      found)
        # /update took GET and POST in http.rs already: the arm of site.rs adds nothing, and needs no entry
        self.assertEqual(len(found), 2, found)

    def test_a_declaration_of_a_trait_is_no_body(self):
        rs = "pub trait Site {\n    fn answer(&mut self, route: &str) -> Option<u8>;\n}\n" + HTTP_RS
        self.assertEqual(cov.routes(rs)[0], cov.routes(HTTP_RS)[0])

    def test_routes_methods_and_tools(self):
        have, arms, findings = cov.routes(HTTP_RS)
        self.assertEqual(findings, [])
        # `if post` is POST, `if !post` GET, none every method, or GET after an arm of its path took POST; the
        # foreign check, the net.allow refusal (GUARDS) and not found are no routes
        self.assertEqual(arms, [("/", {"GET", "POST"}), ("/ui-state", {"GET"}), ("/waveform", {"POST"}),
                                ("/screen.png", {"GET", "POST"}), ("/screen.jpg", {"GET", "POST"}),
                                ("/wave.csv", {"GET", "POST"}), ("/update", {"POST"}), ("/update", {"GET"}),
                                ("/mcp", {"GET", "POST"})])
        self.assertEqual(have["/update"], {"GET", "POST"})
        # the name of serverInfo is no tool
        self.assertEqual(cov.tools(HTTP_RS), (["scpi", "screenshot"], ["scpi", "screenshot"]))

    def test_a_complete_tree_has_no_finding(self):
        self.assertEqual(self.findings(), [])
        names = [name for name, _ in cov.check(str(self.root))[1]]
        self.assertEqual(len(names), 11, names)
        self.assertNotIn("Not an entry", " ".join(names))

    def test_a_new_route_without_docs_fails(self):
        rs = HTTP_RS.replace('"/" => ("text/html"', '"/version" => ("text/plain", b"1.0".to_vec()),\n'
                                                    '            "/" => ("text/html"')
        self.write(rs, HTTP_MD)
        self.assertEqual(self.findings(), [
            "ui/remote/src/http.rs: route GET/POST /version has no entry `### \\`GET /version\\`` in docs/http.md",
            "ui/remote/src/http.rs: route /version has no arm in fn methods_of: it would take every method"])
        # with its entry and its arm of methods_of it passes
        rs = rs.replace('"/" | "/ui-state"', '"/version" | "/" | "/ui-state"')
        self.write(rs, HTTP_MD + "\n### `GET /version`\n\n- Mark: **experimental**\n")
        self.assertEqual(self.findings(), [])

    def test_methods_of_takes_the_methods_of_the_entries(self):
        # a method more or less than the entries name, a HEAD without a GET
        rs = HTTP_RS.replace('"/update" => Some("GET, HEAD, POST")', '"/update" => Some("GET, HEAD")')
        rs = rs.replace('"/screen.png" | "/screen.jpg" => Some("GET")',
                        '"/screen.png" | "/screen.jpg" => Some("GET, POST")')
        rs = rs.replace('"/waveform" | "/mcp" => Some("POST")', '"/waveform" | "/mcp" => Some("HEAD, POST")')
        self.write(rs, HTTP_MD)
        self.assertEqual(self.findings(), [
            "ui/remote/src/http.rs: fn methods_of takes HEAD on /mcp without GET",
            "ui/remote/src/http.rs: fn methods_of lets GET, POST through to /screen.jpg, its entries in docs/http.md "
            "name GET",
            "ui/remote/src/http.rs: fn methods_of lets GET, POST through to /screen.png, its entries in docs/http.md "
            "name GET",
            "ui/remote/src/http.rs: fn methods_of lets GET through to /update, its entries in docs/http.md name GET, "
            "POST",
            "ui/remote/src/http.rs: fn methods_of takes HEAD on /waveform without GET"])
        # the methods are split as the server splits them: "GET,HEAD" is one method it knows of no request
        self.write(HTTP_RS.replace('"/" | "/ui-state" | "/wave.csv" => Some("GET, HEAD")',
                                   '"/" | "/ui-state" | "/wave.csv" => Some("GET,HEAD")'), HTTP_MD)
        self.assertIn("ui/remote/src/http.rs: fn methods_of lets GET,HEAD through to /, its entries in docs/http.md "
                      "name GET", self.findings())
        # an arm in a comment is no arm
        self.write(HTTP_RS.replace('"/waveform" | "/mcp" => Some("POST"),',
                                   '// e.g. "/bogus" => Some("POST"),\n'
                                   '        /* "/x" => Some("GET"), */ "/waveform" | "/mcp" => Some("POST"),'), HTTP_MD)
        self.assertEqual(self.findings(), [])
        # without the function every route would take every method
        self.write(HTTP_RS.replace("fn methods_of(", "fn methods(", 1), HTTP_MD)
        self.assertEqual(self.findings(), ["ui/remote/src/http.rs: no arm found in fn methods_of"])

    def test_the_get_of_a_path_whose_post_is_documented_still_needs_its_own_entry(self):
        self.write(HTTP_RS, HTTP_MD.replace("### `GET /update`\n\n- Mark: **internal**\n\n", ""))
        self.assertEqual(self.findings(), [
            "ui/remote/src/http.rs: route GET /update has no entry `### \\`GET /update\\`` in docs/http.md",
            "ui/remote/src/http.rs: fn methods_of lets GET, POST through to /update, its entries in docs/http.md name "
            "POST"])

    def test_an_entry_without_one_known_mark_fails(self):
        md = HTTP_MD.replace("### `GET /`\n\n- Mark: **internal**", "### `GET /`\n\nThe page.")
        md = md.replace("- Mark: **experimental** (may change)", "- Mark: **frozen**")
        md = md.replace("### `GET /wave.csv`\n\n- Mark: **stable**", "### `GET /wave.csv`\n\n- Mark: **stable**\n"
                                                                     "- Mark: **internal**")
        self.write(HTTP_RS, md)
        found = self.findings()
        self.assertEqual(len(found), 3, found)
        self.assertIn("GET / needs one line `- Mark: **stable**` (or experimental, internal), has none", found[0])
        self.assertIn("GET /ui-state needs one line", found[1])
        self.assertTrue(found[1].endswith("has frozen"), found[1])
        self.assertTrue(found[2].endswith("has stable, internal"), found[2])

    def test_an_entry_the_code_does_not_answer_fails(self):
        md = HTTP_MD + "\n### `GET /gone`\n\n- Mark: **stable**\n\n### `POST /ui-state`\n\n- Mark: **stable**\n" \
                       "\n### MCP tool `nope`\n\n- Mark: **stable**\n\n### `GET /wave.csv`\n\n- Mark: **stable**\n"
        self.write(HTTP_RS, md)
        found = self.findings()
        self.assertEqual(len(found), 5, found)
        self.assertIn("GET /gone: no such route in fn answer of ui/remote/src/http.rs", found[0])
        self.assertIn("POST /ui-state: fn answer of ui/remote/src/http.rs and ui/src/site.rs takes only GET on /ui-state", found[1])
        self.assertIn("MCP tool `nope`: no such tool in fn mcp", found[2])
        self.assertIn("GET /wave.csv has a second entry", found[3])
        self.assertIn("fn methods_of lets GET through to /ui-state, its entries in docs/http.md name GET, POST",
                      found[4])

    def test_an_unknown_guard_fails_rather_than_being_passed_over(self):
        rs = HTTP_RS.replace('"/" => ("text/html"', '"/secret" if peer_ok => ("text/plain", Vec::new()),\n'
                                                    '            "/" => ("text/html"')
        self.write(rs, HTTP_MD)
        self.assertEqual(self.findings(), ["ui/remote/src/http.rs: arm /secret has a guard this check does not know: "
                                           "`if peer_ok` (GUARDS)"])

    def test_a_path_added_to_the_arm_of_a_known_guard_is_not_passed_over(self):
        rs = HTTP_RS.replace('"/update" | "/mcp" if (post', '"/update" | "/mcp" | "/admin" if (post')
        self.write(rs, HTTP_MD)
        self.assertEqual(self.findings(), [
            "ui/remote/src/http.rs: arm /update | /mcp | /admin has the guard `if (post || route == \"/mcp\") && !peer_ok`, "
            "which is known for /mcp | /update only (GUARDS)"])

    def test_tools_need_an_entry_and_the_same_name_in_list_and_call(self):
        self.write(HTTP_RS, HTTP_MD.replace("### MCP tool `screenshot`\n\n- Mark: **stable**\n", ""))
        self.assertEqual(self.findings(), [
            "ui/src/site.rs: MCP tool `screenshot` has no entry `### MCP tool \\`screenshot\\`` in docs/http.md"])
        rs = HTTP_RS.replace('Some("screenshot") => text("png"),', 'Some("screenshot") => text("png"),\n'
                                                                    '            Some("hidden") => text("x"),')
        self.write(rs, HTTP_MD + "\n### MCP tool `hidden`\n\n- Mark: **internal**\n")
        self.assertEqual(self.findings(), [
            "ui/src/site.rs: MCP tool `hidden` is answered by tools/call but not in tools/list"])

    def test_main_exits_1_on_a_finding_and_0_on_this_tree(self):
        self.write(HTTP_RS, HTTP_MD.replace("### `POST /mcp`\n\n- Mark: **stable**\n", ""))
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            self.assertEqual(cov.main([str(self.root)]), 1)
        self.assertIn("route GET/POST /mcp has no entry", out.getvalue())
        self.assertIn("fn methods_of lets POST through to /mcp, its entries in docs/http.md name none", out.getvalue())
        self.assertIn("2 findings", err.getvalue())
        # the tree itself: every route and tool of ui/remote/src/http.rs has its entry and mark in docs/http.md
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            self.assertEqual(cov.main([str(ROOT)]), 0, out.getvalue())


if __name__ == "__main__":
    unittest.main()
