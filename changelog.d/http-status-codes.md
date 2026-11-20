Interface:

- `POST /update` (stable) answers a refused package with 409 instead of 200 (the body still says `REFUSED: <reason>`),
  and `POST /waveform` (stable) answers a request without a slot from 1 to 10 with 400 instead of 409. A client can
  tell from the status that nothing was installed or loaded ([docs/http.md](docs/http.md)).
