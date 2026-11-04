"""Engine IPC de teste: processos reais, sem captura, GPU ou acesso a rede."""
import json
import sys
import time


def emit(event, **fields):
    print(json.dumps(dict(event=event, **fields)), flush=True)


emit("ready", version=4, engine="supervisor-fixture")
command = json.loads(sys.stdin.readline())
request = {key: value for key, value in command.items() if key != "cmd"}
source = request["source"]
if source.get("hwnd") == 789:
    emit("error", code="capture", message="fonte de teste indisponivel")
    sys.exit(1)
if source.get("hwnd") == 321:
    time.sleep(0.4)
quality = request["quality"]
height = quality["height"] or 1080
emit("started", source="fixture", width=height * 16 // 9, height=height,
     source_width=1920, source_height=1080, fps_target=quality["fps"],
     bitrate_target_bps=1000000, publishing=False)
video = "active"
audio_muted = False
for line in sys.stdin:
    command = json.loads(line)
    if command.get("cmd") == "privacy":
        if command["operation"]["sequence"] == 99:
            continue  # Contraprova determinística de timeout do supervisor.
        time.sleep(0.08)
        action = command["action"]
        if action == "pause":
            video = "paused"
        elif action == "resume":
            video = "active"
        elif action == "set_audio_muted":
            audio_muted = command["muted"]
        emit("privacy", operation=command["operation"], video=video, audio_muted=audio_muted)
    if command.get("cmd") == "stop":
        # Abre uma janela deterministica para cancelar a troca antes da proxima partida.
        time.sleep(0.15)
        emit("stopped", reason="requested")
        break
