import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile, mkdtemp, mkdir, writeFile, rm } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { join, resolve, sep } from 'node:path';

const script = resolve('tools/release/installer-smoke.ps1');
const ps = process.platform === 'win32'
  ? join(process.env.SystemRoot, 'System32/WindowsPowerShell/v1.0/powershell.exe') : 'pwsh';
const available = !spawnSync(ps, ['-NoProfile', '-Command', '$PSVersionTable.PSVersion.Major'], {windowsHide:true}).error;
const quote = value => `'${value.replaceAll("'", "''")}'`;
function pure(expression) {
  // Load only pure function ASTs, NEVER dot-source the executable entry point.
  const command = `
    $ErrorActionPreference='Stop'; Set-StrictMode -Version Latest
    $tokens=$null; $errors=$null
    $ast=[System.Management.Automation.Language.Parser]::ParseFile(${quote(script)},[ref]$tokens,[ref]$errors)
    if($errors.Count){throw 'PowerShell parse error'}
    $names=@('Test-SmokePermission','Get-SmokeArguments','Convert-SmokeVersion','Compare-SmokePayload','Get-SmokeContract','Get-SmokeSidecarDiagnostic','Assert-Smoke','Get-SmokeTree','Get-SmokeHash','Get-SmokePayloadEvidence','Get-SmokeErrorDetail')
    foreach($fn in $ast.FindAll({param($a) $a -is [System.Management.Automation.Language.FunctionDefinitionAst]},$true)) {
      if($names -contains $fn.Name){. ([scriptblock]::Create($fn.Extent.Text))}
    }
    & { ${expression} } | ConvertTo-Json -Depth 10 -Compress
  `;
  const result=spawnSync(ps,['-NoProfile','-NonInteractive','-EncodedCommand',Buffer.from(command,'utf16le').toString('base64')],{windowsHide:true,encoding:'utf8',timeout:15000});
  assert.equal(result.status,0,result.stderr);
  return JSON.parse(result.stdout.trim());
}

test('PowerShell 5.1 parses the script and safety requires exact CI or explicit opt-in', {skip:!available},()=>{
  assert.deepEqual(pure(`@( (Test-SmokePermission '' $false), (Test-SmokePermission 'false' $false),
    (Test-SmokePermission 'TRUE' $false), (Test-SmokePermission 'true' $false), (Test-SmokePermission '' $true) )`),
    [false,false,false,true,true]);
});
test('NSIS /D and _?= remain last, unquoted and support spaces', {skip:!available},()=>{
  const target=process.platform==='win32'?String.raw`C:\runner temp\installed`:'/runner temp/installed';
  assert.deepEqual(pure(`@((Get-SmokeArguments ${quote(target)}), (Get-SmokeArguments ${quote(target)} -Uninstall))`),
    [`/S /NS /D=${target}`,`/S _?=${target}`]);
  assert.equal(pure(`try { Get-SmokeArguments 'C:/bad"path'; $false } catch { $true }`),true);
});
test('versions compare numerically without mistaking same-version reinstall for upgrade', {skip:!available},()=>{
  assert.deepEqual(pure(`@( ((Convert-SmokeVersion '0.1.14') -eq (Convert-SmokeVersion '0.1.14.0')),
    ((Convert-SmokeVersion '0.1.9') -lt (Convert-SmokeVersion '0.1.14')), ((Convert-SmokeVersion '0.1.14-beta') -eq (Convert-SmokeVersion '0.1.14')) )`),[true,true,true]);
  assert.equal(pure(`try { Convert-SmokeVersion 'not-a-version'; $false } catch { $true }`),true);
});
test('payload detects stale DLLs, missing files and wrong bytes; only generated uninstaller is exempt', {skip:!available},()=>{
  const result=pure(`Compare-SmokePayload @{ 'app.exe'='new'; 'encoder-runtime/new.dll'='new' } @{ 'app.exe'='old'; 'encoder-runtime/obsolete.dll'='old'; 'uninstall.exe'='generated' }`);
  assert.equal(result.ok,false);
  assert.deepEqual(result.missing,['encoder-runtime/new.dll']);
  assert.deepEqual(result.different,['app.exe']);
  assert.deepEqual(result.extra,['encoder-runtime/obsolete.dll']);
  assert.equal(pure(`(Compare-SmokePayload @{'app.exe'='new'} @{'app.exe'='new';'uninstall.exe'='generated'}).ok`),true);
});
test('actual checked-in NSIS/currentUser/resource contract is recognized', {skip:!available},()=>{
  const app=resolve('apps/madscreen-app/src-tauri');
  const result=pure(`Get-SmokeContract (Get-Content ${quote(join(app,'tauri.conf.json'))} -Raw | ConvertFrom-Json) (Get-Content ${quote(join(app,'tauri.windows.conf.json'))} -Raw | ConvertFrom-Json) (Get-Content ${quote(join(app,'windows/installer.nsi'))} -Raw) (Get-Content ${quote(join(app,'windows/hooks.nsh'))} -Raw)`);
  assert.equal(result.mode,'currentUser');
  assert.equal(result.uninstallKey,String.raw`Software\Microsoft\Windows\CurrentVersion\Uninstall\madscreen`);
});
test('payload evidence distinguishes retained old bytes from a separate same-version build', {skip:!available},()=>{
  // Envelope preserva o array de um elemento atraves do pipeline PS5 ConvertTo-Json.
  const result=pure(`@{ evidence = (Get-SmokePayloadEvidence @{'app.exe'='packaged'} @{'app.exe'='previous'} @{'app.exe'='previous'}) }`);
  assert.equal(result.evidence[0].retainedPrevious,true);
  assert.equal(result.evidence[0].packagedSha256,'packaged');
  const replaced=pure(`@{ evidence = (Get-SmokePayloadEvidence @{'app.exe'='packaged'} @{'app.exe'='packaged'} @{'app.exe'='previous'}) }`);
  assert.equal(replaced.evidence[0].retainedPrevious,false);
});
test('exception detail preserves the real reason and redacts known, drive and UNC paths', {skip:!available},()=>{
  const result=pure(String.raw`try { throw 'exit code 17: C:\Users\owner\private.exe; \\server\personal\secret.txt' } catch { Get-SmokeErrorDetail $_ 'uninstall-failed' @('C:\Users\owner') }`);
  assert.match(result.message,/exit code 17/);
  assert.equal(result.errorType,'RuntimeException');
  assert.ok(result.scriptLine>0);
  assert.doesNotMatch(JSON.stringify(result),/owner|server|secret|private/);
});
test('sidecar failures preserve useful codes without copying personal paths into JSON', {skip:!available},()=>{
  const result=pure(`Get-SmokeSidecarDiagnostic 'D3D11 0x887A0004 C:/Users/private/file.exe'`);
  assert.equal(result.code,'d3d11-unavailable'); assert.equal(result.hresult,'0x887A0004');
  assert.equal(JSON.stringify(result).includes('private'),false);
});
test('snapshot fixture hashes nested payload using only relative names, without executing any file', {skip:!available||process.platform!=='win32'},async()=>{
  const dir=await mkdtemp(join(tmpdir(),'installer-smoke-tree-'));
  try {
    const payload=join(dir,'payload'), extracted=join(dir,'extracted'), zip=join(dir,'reference.zip');
    await mkdir(join(payload,'encoder-runtime'),{recursive:true});
    await writeFile(join(payload,'encoder-runtime','fake.dll'),'not a library');
    const result=pure(`Add-Type -AssemblyName System.IO.Compression.FileSystem;
      [IO.Compression.ZipFile]::CreateFromDirectory(${quote(payload)},${quote(zip)});
      [IO.Compression.ZipFile]::ExtractToDirectory(${quote(zip)},${quote(extracted)});
      Get-SmokeTree ${quote(extracted)}`);
    assert.deepEqual(result,{'encoder-runtime/fake.dll':createHash('sha256').update('not a library').digest('hex')});
  } finally {
    assert.ok(resolve(dir).startsWith(resolve(tmpdir())+sep));
    await rm(dir,{recursive:true,force:true});
  }
});
test('full script refuses outside CI before inspecting installer or launching anything, and writes refusal JSON', {skip:!available},async()=>{
  const dir=await mkdtemp(join(tmpdir(),'installer-smoke-refusal-'));
  try {
    const env={...process.env}; delete env.GITHUB_ACTIONS;
    const target=join(dir,'must-not-exist');
    const result=spawnSync(ps,['-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',script,
      '-Installer',join(dir,'never-executed.exe'),'-WorkDir',target],{env,windowsHide:true,encoding:'utf8',timeout:15000});
    assert.equal(result.status,2,result.stderr);
    const json=JSON.parse(result.stdout.trim()); assert.equal(json.ok,false); assert.equal(json.steps[0].stage,'safety');
    assert.equal(result.stdout.includes(dir),false);
    const report=JSON.parse((await readFile(join(target,'result.json'),'utf8')).replace(/^\uFEFF/,''));
    assert.equal(report.steps[0].stage,'safety'); assert.equal(report.ok,false);
  } finally {
    assert.ok(resolve(dir).startsWith(resolve(tmpdir())+sep));
    await rm(dir,{recursive:true,force:true});
  }
});
test('workflow gate is after NSIS, before publication, with always-uploaded JSON and no bypass',async()=>{
  const workflow=(await readFile('.github/workflows/app.yml','utf8')).replace(/\r\n/g,'\n');
  const begin=workflow.indexOf('- name: Instalacao, upgrade e desinstalacao em maquina limpa');
  const end=workflow.indexOf('- name: Manifesto e verificacao criptografica',begin);
  assert.ok(begin>workflow.indexOf('- name: Instalador NSIS')&&end>begin);
  const step=workflow.slice(begin,end);
  assert.match(step,/gh release download/); assert.match(step,/GH_TOKEN: \$\{\{ github.token \}\}/);
  assert.match(step,/powershell\.exe/); assert.doesNotMatch(step,/-IUnderstandThisInstalls/);
  assert.match(step,/always\(\)/); assert.match(step,/actions\/upload-artifact@v4/); assert.match(step,/result\.json/);
});
test('retry workflow consumes an installer artifact without build or installation bypass',async()=>{
  const workflow=(await readFile('.github/workflows/installer-smoke.yml','utf8')).replace(/\r\n/g,'\n');
  assert.match(workflow,/workflow_dispatch:/); assert.match(workflow,/run_id:/);
  assert.match(workflow,/gh run download/); assert.match(workflow,/actions: read/);
  assert.match(workflow,/madscreen-installer-smoke-input/); assert.match(workflow,/always\(\)/);
  assert.doesNotMatch(workflow,/cargo |npm |npx |-IUnderstandThisInstalls/);
  const app=(await readFile('.github/workflows/app.yml','utf8')).replace(/\r\n/g,'\n');
  assert.ok(app.indexOf('name: madscreen-installer-smoke-input')<app.indexOf('- name: Instalacao, upgrade'));
});
test('reference comes from clean installer payload and uninstall respects NSIS preserved preferences',async()=>{
  const text=(await readFile(script,'utf8')).replace(/\r\n/g,'\n');
  assert.match(text,/source = 'new-installer-clean-install'/);
  assert.match(text,/filesMatchNewInstaller = \$true/);
  assert.doesNotMatch(text,/filesMatchBuildAndRuntime|\$expected\[\$contract.main\] = Get-SmokeHash/);
  const removal=text.slice(text.indexOf('function Remove-SmokeInstallation'),text.indexOf('\ntry {\n    Assert-Smoke'));
  assert.match(removal,/installLocationPreferencePreserved/);
  assert.doesNotMatch(removal,/foreach \(\$key in @\([^\n]*locationKey/);
  const template=(await readFile('apps/madscreen-app/src-tauri/windows/installer.nsi','utf8')).replace(/\r\n/g,'\n');
  assert.match(template,/\$DeleteAppDataCheckboxState = 1[\s\S]*DeleteRegKey SHCTX "\$\{MANUPRODUCTKEY\}"/);
});
