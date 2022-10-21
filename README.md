# NLX gateway

## Project status

Consider this project to be nothing more than a proof of concept.
Some features work but a lot of things aren't (properly) implemented.

## Features

### Inway

- [x] Announce services to Directory
- [x] Register Inway in NLX Management
- [x] HTTP service proxy
- [ ] Graceful shutdown
- [ ] NLX Management API Proxy
- [ ] Delegation
- [ ] Access requests

### Outway

TODO

## Performance

In a minimal test setup the NLX Gateway allocates ~ 7.5 MB or memory.
The Inway proxy overhead is yet to be determined (there are no benchmarks yet).
