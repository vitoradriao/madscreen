"""Executa o fluxo NSIS real com radios/desinstalador externos substituídos.

Não lê o registro do aplicativo, não fecha processos e não instala/desinstala.
"""
import os
from pathlib import Path
import re
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
NSIS = Path(os.environ.get("LOCALAPPDATA", "")) / "tauri/NSIS/makensis.exe"
TEMPLATE = ROOT / "apps/madscreen-app/src-tauri/windows/installer.nsi"


@unittest.skipUnless(NSIS.is_file(), "NSIS local necessário")
class MaintenanceFlow(unittest.TestCase):
    def run_case(self, *, selected=0, version=0, passive=0, update=0, wix=0,
                 allow="true", uninstall_result=0):
        source = TEMPLATE.read_text(encoding="utf-8")
        body = source.split("Function PageLeaveReinstall\n", 1)[1].split("FunctionEnd", 1)[0]
        body = body.replace("${NSD_GetState} $R3 $R1", f"StrCpy $R1 {selected}")
        body = re.sub(r"ReadRegStr[^\n]+", "; Registro isolado pelo harness", body)
        body = re.sub(r"ExecWait[^\n]+", lambda _: f'FileWrite $TestLog "uninstall$\\n"\n    StrCpy $0 {uninstall_result}', body)
        body = re.sub(r'MessageBox[^\n]+', lambda _: 'FileWrite $TestLog "error$\\n"', body)
        with tempfile.TemporaryDirectory(prefix="madscreen-nsis-") as temp:
            directory = Path(temp)
            output = directory / "flow.exe"
            log = directory / "flow.log"
            script = f'''Unicode true
!include LogicLib.nsh
!include nsDialogs.nsh
!define ALLOWDOWNGRADES "{allow}"
!define UNINSTKEY "Unused"
!define MANUPRODUCTKEY "Unused"
!define MAINBINARYNAME "never-created"
Name "madscreen NSIS flow test"
OutFile "{output}"
RequestExecutionLevel user
SilentInstall silent
Var PassiveMode
Var UpdateMode
Var WixMode
Var MaintenanceUninstallOnly
Var TestLog
LoadLanguageFile "${{NSISDIR}}\\Contrib\\Language files\\English.nlf"
LangString unableToUninstall 1033 "test error"
Function PageLeaveReinstall
{body}
FunctionEnd
Function .onInit
  StrCpy $INSTDIR "{directory}"
  StrCpy $PassiveMode {passive}
  StrCpy $UpdateMode {update}
  StrCpy $WixMode {wix}
  StrCpy $R0 {version}
  FileOpen $TestLog "{log}" w
  Call PageLeaveReinstall
  FileWrite $TestLog "continue-install$\\n"
  FileClose $TestLog
  Quit
FunctionEnd
Section
SectionEnd
'''
            path = directory / "flow.nsi"
            path.write_text(script, encoding="utf-8")
            result = subprocess.run([str(NSIS), "/V2", str(path)], capture_output=True, timeout=30)
            self.assertEqual(result.returncode, 0, result.stdout.decode(errors="replace") + result.stderr.decode(errors="replace"))
            subprocess.run([str(output), "/S"], check=False, timeout=15)
            return log.read_text().splitlines()

    def test_update_keeps_existing_install(self):
        for version in (0, 1, -1):
            with self.subTest(version=version):
                self.assertEqual(self.run_case(version=version), ["continue-install"])

    def test_uninstall_never_reinstalls(self):
        for version in (0, 1, -1):
            with self.subTest(version=version):
                self.assertEqual(self.run_case(selected=1, version=version), ["uninstall"])

    def test_passive_and_update_do_not_use_skipped_controls(self):
        self.assertEqual(self.run_case(selected=1, passive=1), ["continue-install"])
        self.assertEqual(self.run_case(selected=1, update=1), ["continue-install"])

    def test_wix_migration_and_explicit_uninstall(self):
        self.assertEqual(self.run_case(wix=1), ["uninstall", "continue-install"])
        self.assertEqual(self.run_case(wix=1, passive=1), ["uninstall", "continue-install"])
        self.assertEqual(self.run_case(wix=1, selected=1), ["uninstall"])

    def test_failure_does_not_install(self):
        self.assertEqual(self.run_case(selected=1, uninstall_result=2), ["uninstall", "error"])

    def test_blocked_downgrade_does_not_install(self):
        self.assertEqual(self.run_case(version=-1, allow="false"), [])
        self.assertEqual(self.run_case(selected=1, version=-1, allow="false"), ["uninstall"])


if __name__ == "__main__":
    unittest.main()
