# LASHA
Linux ASHA driver app!

App is still in early developement and has not been confirmed to work on any hearing aid or cochlear implant processor. 


# Building

To build this project, you must have the latest stable version of Rust.

1. Clone the repo.
2. Change your working directory to the cloned repo.
3. Build and run with `cargo run`

# Usage
1. Bluetooth must be enabled
2. Your audio processor(s) must be connected to your computer before it will try streaming (only one device is recommended).
3. Have audio playing on the system at half volume. It is recommended that you route to the headphone jack with nothing connected. In the future, the app will appear as it's own audio device. 