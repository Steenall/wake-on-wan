# Wake on wan
This application create a server and send wake on lan signals to your devices.
## Compile and running the server
To compile this project, you can use the ```cargo build``` command or the ```cargo run``` command if you want to run it directly.
By default, the server is launched on the port 44844. To test it, you can just launch the server and go to your favorite browser locally.
```http://localhost:44844```
You will need to replace the example placed inside the ```computer_to_wake.csv``` file in order to try it yourself.

## Add a device
To add a device, you will need to edit the ```computer_to_wake.csv``` file. An example has been added to this file. You will need the MAC adress, the ip and the port and write it in the file with this pattern :
```csv
MAC_ADRESS;IP;PORT
```
To add another device, you can just make a new line and use the same pattern.


