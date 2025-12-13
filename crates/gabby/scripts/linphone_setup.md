# Linphone Setup for Gabby

This guide explains how to configure Linphone to call Gabby.

## Installation

### Linux (Debian/Ubuntu)
```bash
sudo apt update
sudo apt install linphone
```

### macOS
```bash
brew install --cask linphone
```

### Windows
Download from: https://www.linphone.org/releases/windows/app/

### Mobile
- iOS: App Store - search "Linphone"
- Android: Play Store - search "Linphone"

## Configuration

### Step 1: Create a Direct SIP Account

Linphone normally expects you to register with a SIP provider, but for calling Gabby directly we'll create a local account.

1. Open Linphone
2. Go to **Settings** (gear icon) > **SIP accounts**
3. Click **Add account** or **+**
4. Select **I already have a SIP account**
5. Enter these details:
   - **Username**: `caller` (or anything you like)
   - **SIP Domain**: Enter your computer's IP address (e.g., `192.168.1.100`)
   - **Password**: Leave blank
   - **Transport**: Select **UDP**
6. Click **Add** or **Save**

### Step 2: Disable Registration

Since Gabby accepts calls directly without a SIP registrar:

1. Go to the account settings you just created
2. Find and disable **Register** option
3. Set **Registration expires** to 0

### Step 3: Configure Audio Codecs

Gabby uses G.711 mu-law (PCMU) codec at 8kHz.

1. Go to **Settings** > **Audio**
2. Find **Codecs** section
3. **Enable** these codecs (set as highest priority):
   - PCMU (G.711 mu-law)
   - PCMA (G.711 A-law)
4. **Disable** other codecs to ensure compatibility:
   - Opus
   - G.722
   - Speex
   - etc.

### Step 4: Check Audio Devices

1. Go to **Settings** > **Audio**
2. Select your **Microphone** (input device)
3. Select your **Speaker** (output device)
4. Optionally run an echo test to verify audio works

## Making a Call to Gabby

### Option 1: Dial Directly

1. In the main Linphone window, find the dial pad or address bar
2. Enter the SIP URI:
   ```
   sip:gabby@<gabby-server-ip>:5060
   ```
   For example:
   ```
   sip:gabby@192.168.1.100:5060
   ```
3. Click the **Call** button (phone icon)

### Option 2: Add as Contact

1. Go to **Contacts**
2. Add a new contact:
   - **Name**: Gabby
   - **SIP Address**: `sip:gabby@<gabby-server-ip>:5060`
3. Save and call from contacts

## What to Expect

1. **Ringing**: You should see "Ringing" status
2. **Connected**: Gabby will answer and greet you
3. **Conversation**:
   - Speak naturally into your microphone
   - Pause for ~1 second when done speaking
   - Gabby will process your speech and respond
4. **Hang up**: Click the red hang up button when done

## Troubleshooting

### "No route to host" or "Connection refused"

- Verify Gabby is running: check the console output
- Check the IP address is correct
- Ensure firewall allows UDP port 5060 and 10000-20000

### No audio heard

- Check your speaker volume
- Verify correct audio device is selected in Linphone
- Make sure only G.711 codecs are enabled
- Check if TTS (Piper) is installed on the Gabby server

### Gabby doesn't understand speech

- Speak clearly and at normal pace
- Reduce background noise
- Ensure your microphone is working (test in Linphone settings)
- Check if Vosk model is loaded in Gabby console

### Call drops immediately

- Check Gabby console for errors
- Verify UDP port 5060 is not blocked
- Try restarting both Linphone and Gabby

### Gabby doesn't respond to questions

- Verify Ollama is running: `curl http://localhost:11434/api/tags`
- Check if the model is loaded: `ollama list`
- Look for errors in Gabby's console output

## Alternative SIP Clients

If Linphone doesn't work for you, try these alternatives:

### Desktop
- **Ooh**: [Zoiper](https://www.zoiper.com/) - Free, cross-platform
- **Twinkle**: Linux only, lightweight
- **MicroSIP**: Windows only, very simple

### Mobile
- **Ooh**: Zoiper (iOS/Android)
- **Ooh**: Ooh SIP (iOS/Android)

### Web Browser
- **SIP.js Demo**: https://sipjs.com/demo/

## Network Requirements

For Gabby to receive calls, ensure these ports are open:

| Port | Protocol | Purpose |
|------|----------|---------|
| 5060 | UDP | SIP signaling |
| 10000-20000 | UDP | RTP audio |

If using a firewall:
```bash
# UFW (Ubuntu)
sudo ufw allow 5060/udp
sudo ufw allow 10000:20000/udp

# firewalld (Fedora/RHEL)
sudo firewall-cmd --add-port=5060/udp --permanent
sudo firewall-cmd --add-port=10000-20000/udp --permanent
sudo firewall-cmd --reload
```
