# MiBand Heart Rate RESTful
OpenDR Team Internal Codename:Hagu([how it came about](https://projectsekai.fandom.com/wiki/Hug))
> A miband heart rate branch that supports RESTful API and WebSocket services   
> Powered by Actix
> 
Supported platform and supported MiBands see origin project :[MIBand Heart Rate Demo](https://github.com/Tnze/miband-heart-rate)
# Quick Start
Download the latest release from [GitHub Releases](https://github.com/YuxiangWang0525/miband-heart-rate-RESTful/releases)  
1. Create a folder named `static` in the same directory as `miband-heart-rate.exe`
2. Run `miband-heart-rate.exe`, program will listening on port 28040
3. Turn on the heart rate broadcast function on your Miband and select any sport (recommended "free activity")
4. Waiting for the program to search for the wristband BLE broadcast. 
> When the connection is successful, the console will output heart rate information. At the same time, MiBand will vibrate
5. Put the frontend files in the static directory,and add browser sources to streaming software such as OBS. Set as address http://localhost:28040(optional)
# API Document
Endpoint: [host]:28040  
## GET /api/heart-rate
Get heart rate data from the last record.  
JSON

| Parameter | Type | Description |
| --- | --- | --- |
| `value` | `int` | last record of heart rate |
| `sensor_contact_detected` | `"string"` | - |
| `timestamp` | `int` | timestamp of last record |
## WebSocket /api/ws
WebSocket service for real-time heart rate data.
> The heart rate data is sent to the client when a new record is received.
The content is consistent with /api/heart-rate

# About frontend file
I have prepared a small component frontend for reference:[heart-rate-widget-projectsekai](https://github.com/YuxiangWang0525/heart-rate-widget-projectsekai).  
It is also a template repository You can use it to create your own widgets