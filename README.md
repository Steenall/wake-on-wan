# Wake on wan

This application create a server and send wake on lan signals to your devices.
The goal of this project is to have a barebone minimalist server running on a slow/old hardware.

## Compile and running the server

To compile this project, you can use the ```cargo build``` command or the ```cargo run``` command if you want to run it directly.
By default, the server is launched on the port 44844. To test it, you can just launch the server and go to your favorite browser locally.
```http://localhost:44844```
You will need to replace the example placed inside the ```computer_to_wake.csv``` file in order to try it yourself.

## Server settings

The server reads its request limits from ```config.properties``` at startup:
```properties
# Maximum accepted requests from one client IP during each one-second window.
max_requests_per_ip_per_second=1

# Maximum time allowed to receive a request, in milliseconds.
max_request_duration_ms=500

# TCP port the server listens on.
tcp_listener_port=44844

```
Logs are printed to the console with local timestamps and timezone offsets.
Logs are put in the log folder and created on startup if it doesn't exist
Debug-level messages alone are appended to ```debug.log```, errors are appended to ```error.log```, and requests are appended to ```requests.log```

## Add a device

To add a device, edit ```computer_to_wake.csv``` using this pattern:
```csv
NAME;MAC_ADDRESS;IP;PORT
```
Each name must be non-empty and unique. Use the name in the path to wake one computer (for example, ```GET /computer```)
