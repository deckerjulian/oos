Interface:

- The web server tells the methods of HTTP apart: each route takes the method of its reference (and `HEAD` where it
  takes `GET`, but `/cmd`), another method gets 405 with an `Allow` header, and a `HEAD` gets no body. Before, every
  method but `POST` was taken as `GET`, so `POST /wave.csv` gave the CSV and `GET /mcp` an MCP parse error
  ([docs/http.md](docs/http.md)).
