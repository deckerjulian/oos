Interface:

- `GET /cmd` (experimental): a request with one command a remote may not give is refused as a whole, and none of its
  commands runs. Before, the allowed ones ran anyway ([docs/http.md](docs/http.md)).
