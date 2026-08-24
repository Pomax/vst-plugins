<#
.SYNOPSIS
Screenshot the real editor window.

.DESCRIPTION
The headless snapshot renderer proves the drawing code is right, but it does not
prove the actual window is: that path goes through baseview and OpenGL, and the
background is the renderer's clear colour rather than anything egui draws. This
launches the real preview window, waits for it to appear, captures its pixels
off the screen with BitBlt, and closes it.

.EXAMPLE
powershell -ExecutionPolicy Bypass -File src/tools/capture-window.ps1 -Theme light -Out target/window-light.png
#>
param(
    [ValidateSet('light', 'dark', 'auto')]
    [string]$Theme = 'auto',
    [string]$Out = 'target/window.png',
    [string]$Exe = 'target/debug/examples/preview.exe',
    [string]$Notes = '',
    # Arguments to launch with. Given, they replace -Theme and -Notes, for an
    # executable that does not take those. Separate them with `|`: with
    # `powershell -File`, only one token binds to a parameter.
    [string]$ExeArgs = '',
    [string]$Title = 'Notepad',
    # "x,y" inside the window to click before typing, so the keys go where a
    # user's would.
    [string]$ClickAt = '',
    # Text to type into the window before the grab, so what the keyboard
    # actually reaches can be seen rather than assumed.
    [string]$Type = '',
    # A sequence of actions, run in order before the grab:
    #   click:X,Y     left click at X,Y inside the window
    #   type:TEXT     send TEXT as key presses
    #   wait:MS       pause
    #   remove:PATH   delete a file, so a save does not hit "already exists"
    [string[]]$Steps = @(),
    # The same, one per line, from a file. `powershell -File` cannot bind more
    # than one token to an array parameter, so a sequence comes from here.
    [string]$StepFile = '',
    # Close the window by asking it to, rather than killing the process, so
    # the program runs its shutdown. A UI test needs this: the state it
    # asserts on is written on the way out.
    [switch]$CloseCleanly,
    # "x,y" inside the window to park the pointer on before the grab, for
    # capturing a hover state.
    [string]$HoverAt = '',
    [int]$SettleMs = 2500
)

$ErrorActionPreference = 'Stop'

Add-Type @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public class Win32Capture {
    // Without this the capturing process is DPI-virtualised: window coordinates
    // come back in physical pixels while CopyFromScreen works in scaled ones,
    // and the grab lands offset from the window.
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint x, uint y, uint d, System.IntPtr i);
    [DllImport("user32.dll")] public static extern void keybd_event(byte k, byte s, uint f, System.IntPtr i);

    /// Bring a window to the front, working around the foreground lock.
    ///
    /// Windows only allows a foreground change from a process that has just
    /// had input, so a synthetic Alt tap is sent first. Without it, the second
    /// and later windows in a test run never activate and every keystroke goes
    /// somewhere else.
    public static bool BringToFront(IntPtr hWnd) {
        for (int i = 0; i < 20; i++) {
            keybd_event(0x12, 0, 0, System.IntPtr.Zero);        // Alt down
            keybd_event(0x12, 0, 2, System.IntPtr.Zero);        // Alt up
            SetForegroundWindow(hWnd);
            if (GetForegroundWindow() == hWnd) return true;
            System.Threading.Thread.Sleep(150);
        }
        return false;
    }
    // Keyboard input the way a keyboard sends it.
    //
    // `SendKeys` writes a shortcut as `^a` and `keybd_event` sends a scan code
    // of zero. Neither reliably reaches a window that watches the modifier
    // keys themselves: the letter arrives and the Ctrl does not, so the
    // shortcut is read as the plain letter. `SendInput` with the scan code the
    // key really has is what a keyboard produces.
    [StructLayout(LayoutKind.Sequential)]
    public struct KEYBDINPUT {
        public ushort wVk, wScan;
        public uint dwFlags, time;
        public IntPtr dwExtraInfo;
    }
    [StructLayout(LayoutKind.Sequential)]
    public struct INPUT {
        public uint type;
        public KEYBDINPUT ki;
        // The union is as wide as MOUSEINPUT; keyboard entries leave the rest
        // unread, but the size has to match or SendInput refuses the array.
        public int padding1, padding2;
    }
    [DllImport("user32.dll", SetLastError = true)]
    public static extern uint SendInput(uint count, INPUT[] inputs, int size);
    [DllImport("user32.dll")] public static extern uint MapVirtualKeyW(uint code, uint mapType);

    const uint INPUT_KEYBOARD = 1;
    const uint KEYEVENTF_KEYUP = 0x0002;
    const uint KEYEVENTF_SCANCODE = 0x0008;

    static INPUT KeyInput(ushort vk, bool up) {
        var input = new INPUT();
        input.type = INPUT_KEYBOARD;
        input.ki.wVk = vk;
        input.ki.wScan = (ushort)MapVirtualKeyW(vk, 0);   // MAPVK_VK_TO_VSC
        input.ki.dwFlags = up ? KEYEVENTF_KEYUP : 0;
        return input;
    }

    // What the pointer looks like right now. A window says what it wants the
    // cursor to be, and the only way to check it from outside is to ask the
    // system which cursor is on screen and compare it with the stock ones.
    [StructLayout(LayoutKind.Sequential)]
    public struct CURSORINFO {
        public int cbSize, flags;
        public IntPtr hCursor;
        public POINT ptScreenPos;
    }
    [DllImport("user32.dll")] public static extern bool GetCursorInfo(ref CURSORINFO info);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr LoadCursorW(IntPtr instance, int name);

    const int IDC_ARROW = 32512;
    const int IDC_IBEAM = 32513;
    const int IDC_HAND = 32649;

    /// The name of the cursor currently on screen, as far as the stock set goes.
    public static string CursorNow() {
        var info = new CURSORINFO();
        info.cbSize = Marshal.SizeOf(typeof(CURSORINFO));
        if (!GetCursorInfo(ref info)) return "unreadable";
        if (info.hCursor == LoadCursorW(IntPtr.Zero, IDC_IBEAM)) return "ibeam";
        if (info.hCursor == LoadCursorW(IntPtr.Zero, IDC_ARROW)) return "arrow";
        if (info.hCursor == LoadCursorW(IntPtr.Zero, IDC_HAND)) return "hand";
        return "other";
    }

    /// Hold a modifier, tap a key, let go, as a keyboard does it.
    public static void Chord(ushort modifier, ushort key) {
        Send(KeyInput(modifier, false));
        System.Threading.Thread.Sleep(40);
        Send(KeyInput(key, false));
        System.Threading.Thread.Sleep(40);
        Send(KeyInput(key, true));
        System.Threading.Thread.Sleep(40);
        Send(KeyInput(modifier, true));
        System.Threading.Thread.Sleep(60);
    }

    static void Send(INPUT input) {
        var one = new INPUT[] { input };
        SendInput(1, one, Marshal.SizeOf(typeof(INPUT)));
    }

    /// A click the UI can see: the pointer moves, settles, presses, and only
    /// then releases. Sent back to back, the press and release can land in one
    /// frame and be missed.
    public static void Click(int x, int y) {
        SetCursorPos(x, y);
        System.Threading.Thread.Sleep(120);
        mouse_event(0x0002, 0, 0, 0, System.IntPtr.Zero); // left down
        System.Threading.Thread.Sleep(120);
        mouse_event(0x0004, 0, 0, 0, System.IntPtr.Zero); // left up
        System.Threading.Thread.Sleep(120);
    }
    /// Press at one point, move across, release at another: a drag.
    ///
    /// The pointer moves in steps rather than jumping, because a UI works out
    /// what is being selected from where the pointer goes, not from where it
    /// ends up.
    public static void Drag(int fromX, int fromY, int toX, int toY) {
        SetCursorPos(fromX, fromY);
        System.Threading.Thread.Sleep(120);
        mouse_event(0x0002, 0, 0, 0, System.IntPtr.Zero); // left down
        System.Threading.Thread.Sleep(120);
        const int steps = 8;
        for (int i = 1; i <= steps; i++) {
            SetCursorPos(
                fromX + (toX - fromX) * i / steps,
                fromY + (toY - fromY) * i / steps);
            System.Threading.Thread.Sleep(40);
        }
        System.Threading.Thread.Sleep(120);
        mouse_event(0x0004, 0, 0, 0, System.IntPtr.Zero); // left up
        System.Threading.Thread.Sleep(120);
    }

    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    // GetWindowRect gives screen coordinates directly. GetClientRect +
    // ClientToScreen was tried first and returned a bogus size here.
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }

    // A window's outer rect includes the invisible resize border the compositor
    // owns, so a grab of it picks up a strip of whatever is behind. This is the
    // rect that is actually painted.
    [DllImport("dwmapi.dll")]
    public static extern int DwmGetWindowAttribute(IntPtr hWnd, int attr, out RECT value, int size);
    const int DWMWA_EXTENDED_FRAME_BOUNDS = 9;

    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr hWnd, out RECT lpRect);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr hWnd, ref POINT p);
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }

    /// The drawable area, in screen coordinates.
    ///
    /// A dialog's contents start below its title bar, and how tall that is
    /// depends on the theme and the scaling. Measuring from the client rect
    /// takes the question away.
    public static RECT ClientRect(IntPtr hWnd) {
        RECT r;
        GetClientRect(hWnd, out r);
        POINT origin; origin.X = r.Left; origin.Y = r.Top;
        ClientToScreen(hWnd, ref origin);
        int width = r.Right - r.Left, height = r.Bottom - r.Top;
        r.Left = origin.X; r.Top = origin.Y;
        r.Right = origin.X + width; r.Bottom = origin.Y + height;
        return r;
    }

    /// Painted bounds, falling back to the outer rect where DWM has no answer.
    public static RECT VisibleRect(IntPtr hWnd) {
        RECT r;
        int hr = DwmGetWindowAttribute(hWnd, DWMWA_EXTENDED_FRAME_BOUNDS, out r, Marshal.SizeOf(typeof(RECT)));
        if (hr == 0 && r.Right > r.Left && r.Bottom > r.Top) return r;
        GetWindowRect(hWnd, out r);
        return r;
    }

    public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lParam);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    // CharSet.Unicode matters: without it the wide title marshals as ANSI and
    // comes back as just its first character.
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetWindowTextW(IntPtr hWnd, StringBuilder text, int count);



    /// The process's visible window whose title matches, else its largest one.
    /// `MainWindowHandle` is unreliable here: it can name whichever window
    /// happened to be created first, which is not the editor.
    /// When `strict`, only an exact title match counts. The wait loop needs
    /// that: the process also owns a console window that appears first, and
    /// falling back to "largest" immediately would grab it.
    public static IntPtr FindWindow(uint targetPid, string wantedTitle, bool strict) {
        IntPtr match = IntPtr.Zero;
        IntPtr biggest = IntPtr.Zero;
        long biggestArea = 0;
        EnumWindows((h, l) => {
            uint pid;
            GetWindowThreadProcessId(h, out pid);
            if (pid != targetPid || !IsWindowVisible(h)) return true;
            RECT r;
            if (!GetWindowRect(h, out r)) return true;
            long area = (long)(r.Right - r.Left) * (r.Bottom - r.Top);
            var title = new StringBuilder(256);
            GetWindowTextW(h, title, 256);
            if (title.ToString() == wantedTitle) match = h;
            if (area > biggestArea) { biggestArea = area; biggest = h; }
            return true;
        }, IntPtr.Zero);
        if (match != IntPtr.Zero) return match;
        return strict ? IntPtr.Zero : biggest;
    }
}
'@

[Win32Capture]::SetProcessDPIAware() | Out-Null

if (-not (Test-Path $Exe)) {
    throw "$Exe not found - run: cargo build -p notepad-plugin --example preview"
}

$outDir = Split-Path -Parent $Out
if ($outDir -and -not (Test-Path $outDir)) {
    New-Item -ItemType Directory -Force $outDir | Out-Null
}

if ($ExeArgs) {
    $launchArgs = $ExeArgs -split '\|'
} else {
    $launchArgs = @($Theme)
    if ($Notes) { $launchArgs += $Notes }
}

function Save-Shot([Win32Capture+RECT]$area, [string]$path) {
    Add-Type -AssemblyName System.Drawing
    $w = $area.Right - $area.Left
    $h = $area.Bottom - $area.Top
    if ($w -le 0 -or $h -le 0) { throw "bad window rect ${w}x${h}" }
    $bmp = New-Object System.Drawing.Bitmap $w, $h
    $gfx = [System.Drawing.Graphics]::FromImage($bmp)
    # Copy straight off the screen: PrintWindow returns black for GL surfaces.
    $gfx.CopyFromScreen($area.Left, $area.Top, 0, 0,
        (New-Object System.Drawing.Size $w, $h))
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $gfx.Dispose()
    $bmp.Dispose()
}

# Where ffmpeg is. Recording a window is its job, and screenshots cannot do it:
# a flicker lasting a frame or two is gone between two grabs half a second
# apart. gdigrab takes every frame the window draws.
function Get-Ffmpeg {
    $found = Get-Command ffmpeg -ErrorAction SilentlyContinue
    if ($found) { return $found.Source }
    $known = Join-Path $env:ProgramFiles 'ffmpeg\bin\ffmpeg.exe'
    if (Test-Path $known) { return $known }
    throw 'ffmpeg is needed to record a window, and it is not installed'
}

# How many frames of a recording a filter keeps. With no filter that is the
# length of the film; with a scene-change filter it is the number of times the
# picture changed wholesale.
#
# ffmpeg reports through stderr, and redirecting a native program's stderr
# inside PowerShell turns every line of it into an error, so it goes to a file
# and is read back.
$countFilmFrames = {
    param($log, $filter, $video)
    $arguments = @('-hide_banner', '-i', $video)
    if ($filter) { $arguments += @('-vf', $filter) }
    $arguments += @('-f', 'null', '-')
    Start-Process -FilePath (Get-Ffmpeg) -Wait -WindowStyle Hidden `
        -RedirectStandardError $log -ArgumentList $arguments
    $seen = Get-Content -Path $log |
        Select-String 'frame=\s*(\d+)' |
        Select-Object -Last 1
    if ($seen) { [int]$seen.Matches.Groups[1].Value } else { 0 }
}

# The recording in progress, if `film:` has started one.
$film = $null
$filmTo = ''
$filmVideo = ''

# Where the rows of a preset list sit inside the dialog's drawable area. These
# follow the dialog's own layout in `chrome.rs`: the list box border and the
# margin inside it, then one row per preset.
$ROW_X = 60
$ROW_TOP = 3
$ROW_HEIGHT = 21
$BAR_WIDTH = 16

$proc = Start-Process -FilePath $Exe -ArgumentList $launchArgs -PassThru

try {
    # Wait for the editor window to exist.
    $hwnd = [IntPtr]::Zero
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    while ($hwnd -eq [IntPtr]::Zero -and [DateTime]::UtcNow -lt $deadline) {
        Start-Sleep -Milliseconds 200
        $hwnd = [Win32Capture]::FindWindow([uint32]$proc.Id, $Title, $true)
    }
    if ($hwnd -eq [IntPtr]::Zero) {
        # No window titled "Notepad" turned up; take the largest one and say so.
        $hwnd = [Win32Capture]::FindWindow([uint32]$proc.Id, $Title, $false)
        Write-Warning 'no window titled "Notepad" found; falling back to the largest one'
    }
    if ($hwnd -eq [IntPtr]::Zero) { throw 'the preview window never appeared' }
    [Win32Capture]::ShowWindow($hwnd, 5) | Out-Null   # SW_SHOW
    if (-not [Win32Capture]::BringToFront($hwnd)) {
        Write-Warning 'the window would not come to the front'
    }

    # Let the GL context draw a few frames before grabbing pixels.
    Start-Sleep -Milliseconds $SettleMs

    $rect = [Win32Capture]::VisibleRect($hwnd)
    # The screenshot is always of the window under test, whichever window the
    # steps were last clicking in.
    $hostRect = $rect
    # Which window the steps are addressing, for recording it.
    $currentTitle = $Title
    $currentHwnd = $hwnd

    if ($ClickAt) {
        $parts = $ClickAt -split ','
        [Win32Capture]::Click($rect.Left + [int]$parts[0], $rect.Top + [int]$parts[1])
        Start-Sleep -Milliseconds 500
    }

    if ($Type) {
        Add-Type -AssemblyName System.Windows.Forms
        [System.Windows.Forms.SendKeys]::SendWait($Type)
        Start-Sleep -Milliseconds 900
    }

    if ($StepFile) {
        $Steps = Get-Content -Path $StepFile |
            Where-Object { $_.Trim() -ne '' -and -not $_.StartsWith('#') }
    }

    if ($Steps.Count -gt 0) {
        Add-Type -AssemblyName System.Windows.Forms
        foreach ($step in $Steps) {
            $kind, $value = $step -split ':', 2
            switch ($kind) {
                'click' {
                    $parts = $value -split ','
                    [Win32Capture]::Click(
                        $rect.Left + [int]$parts[0], $rect.Top + [int]$parts[1])
                    Start-Sleep -Milliseconds 500
                }
                'type' {
                    [System.Windows.Forms.SendKeys]::SendWait($value)
                    Start-Sleep -Milliseconds 700
                }
                'wait' { Start-Sleep -Milliseconds ([int]$value) }
                'drag' {
                    # `drag:X1,Y1,X2,Y2` presses at the first point, releases at
                    # the second, moving across in between.
                    $parts = $value -split ','
                    if ($parts.Count -ne 4) { throw "cannot read drag: $value" }
                    [Win32Capture]::Drag(
                        $rect.Left + [int]$parts[0], $rect.Top + [int]$parts[1],
                        $rect.Left + [int]$parts[2], $rect.Top + [int]$parts[3])
                    Start-Sleep -Milliseconds 400
                }
                'cursor' {
                    # `cursor:X,Y|ibeam` parks the pointer and checks what the
                    # window asked the cursor to be there.
                    $where, $want = $value -split '\|', 2
                    $parts = $where -split ','
                    [Win32Capture]::SetCursorPos(
                        $rect.Left + [int]$parts[0], $rect.Top + [int]$parts[1]) | Out-Null
                    # The window only changes it when it next redraws.
                    Start-Sleep -Milliseconds 700
                    $shown = [Win32Capture]::CursorNow()
                    if ($shown -ne $want.Trim()) {
                        throw "cursor at $where is $shown, expected $($want.Trim())"
                    }
                }
                'chord' {
                    # `chord:ctrl+a` is a modifier held down over one key.
                    $parts = $value.Trim().ToLower() -split '\+'
                    if ($parts.Count -ne 2) { throw "cannot read chord: $value" }
                    $modifier = switch ($parts[0]) {
                        'ctrl'  { 0x11 }
                        'shift' { 0x10 }
                        'alt'   { 0x12 }
                        default { throw "unknown modifier: $($parts[0])" }
                    }
                    $letter = $parts[1]
                    if ($letter.Length -ne 1) { throw "unknown key: $letter" }
                    $key = [uint16][char]($letter.ToUpper())
                    [Win32Capture]::Chord([uint16]$modifier, $key)
                    Start-Sleep -Milliseconds 300
                }
                'restart' {
                    # Close the program and start it again. What a preset has
                    # to survive is the host going away, so a test that only
                    # ever loads back into the process that saved it is not
                    # testing the thing.
                    if ($hwnd -ne [IntPtr]::Zero) {
                        [Win32Capture]::PostMessage(
                            $hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
                        if (-not $proc.WaitForExit(10000)) {
                            Stop-Process -Id $proc.Id -Force
                        }
                    }
                    # Throw away what the run that just ended wrote, so what is
                    # asserted afterwards can only have come from the new one.
                    # Otherwise a second run that dies quietly leaves the first
                    # run's state behind and the test passes on it.
                    for ($i = 0; $i -lt $launchArgs.Count - 1; $i++) {
                        if ($launchArgs[$i] -eq '--state') {
                            Remove-Item -LiteralPath $launchArgs[$i + 1] `
                                -Force -ErrorAction SilentlyContinue
                        }
                    }
                    $proc = Start-Process -FilePath $Exe -ArgumentList $launchArgs -PassThru
                    $hwnd = [IntPtr]::Zero
                    $deadline = [DateTime]::UtcNow.AddSeconds(30)
                    while ($hwnd -eq [IntPtr]::Zero -and [DateTime]::UtcNow -lt $deadline) {
                        Start-Sleep -Milliseconds 200
                        $hwnd = [Win32Capture]::FindWindow([uint32]$proc.Id, $Title, $true)
                    }
                    if ($hwnd -eq [IntPtr]::Zero) {
                        throw 'the window did not come back after a restart'
                    }
                    [Win32Capture]::ShowWindow($hwnd, 5) | Out-Null
                    if (-not [Win32Capture]::BringToFront($hwnd)) {
                        Write-Warning 'the restarted window would not come to the front'
                    }
                    Start-Sleep -Milliseconds $SettleMs
                    $rect = [Win32Capture]::VisibleRect($hwnd)
                    $hostRect = $rect
                    $currentHwnd = $hwnd
                    $currentTitle = $Title
                }
                'row' {
                    # `row:DIR|NAME` clicks the row for a named preset in the
                    # list. The directory is the real one, shared with whoever
                    # is using the host, so the row a test wants is wherever
                    # the alphabet puts it among files it did not write.
                    #
                    # A row past the bottom of the list is scrolled to first, by
                    # clicking the bottom scroll head, which moves exactly one
                    # row per click.
                    $dir, $name = $value -split '\|', 2
                    $names = @(Get-ChildItem -LiteralPath $dir -Filter *.preset |
                        ForEach-Object { $_.BaseName } | Sort-Object)
                    $index = [Array]::IndexOf($names, $name)
                    if ($index -lt 0) {
                        throw "no preset called ""$name"" in $dir; there is: $($names -join ', ')"
                    }

                    $listHeight = ($rect.Bottom - $rect.Top) - 2 * $ROW_TOP
                    $onScreen = [Math]::Floor($listHeight / $ROW_HEIGHT)
                    $scrollBy = $index - $onScreen + 1
                    $showAt = $index
                    if ($scrollBy -gt 0) {
                        $head = $rect.Right - [int]($BAR_WIDTH / 2)
                        $y = $rect.Bottom - [int]($BAR_WIDTH / 2)
                        for ($i = 0; $i -lt $scrollBy; $i++) {
                            [Win32Capture]::Click($head, $y)
                        }
                        $showAt = $onScreen - 1
                    }
                    [Win32Capture]::Click(
                        $rect.Left + $ROW_X,
                        $rect.Top + $ROW_TOP + $showAt * $ROW_HEIGHT + [int]($ROW_HEIGHT / 2))
                    Start-Sleep -Milliseconds 500
                }
                'copies' {
                    # `copies:SRC|PREFIX|N` makes N copies of a file, named
                    # PREFIX-01 upwards. Filling a list long enough to scroll
                    # does not need the host: one real preset and a copier do.
                    $src, $prefix, $count = $value -split '\|', 3
                    if (-not (Test-Path -LiteralPath $src)) {
                        throw "nothing to copy at $src"
                    }
                    $extension = [System.IO.Path]::GetExtension($src)
                    for ($i = 1; $i -le [int]$count; $i++) {
                        $to = '{0}-{1:d2}{2}' -f $prefix, $i, $extension
                        Copy-Item -LiteralPath $src -Destination $to -Force
                    }
                }
                'window' {
                    # Clicks are relative to a window, and a dialog is a window
                    # of its own. This says which one the ones after it mean.
                    #
                    # A dialog's coordinates are inside its drawable area, so a
                    # step does not have to know how tall a title bar is.
                    # `window:` with no title goes back to the window under
                    # test, whose coordinates include its frame.
                    $wanted = if ($value.Trim() -eq '') { $Title } else { $value.Trim() }
                    $target = [IntPtr]::Zero
                    foreach ($try in 1..20) {
                        $target = [Win32Capture]::FindWindow(
                            [uint32]$proc.Id, $wanted, $true)
                        if ($target -ne [IntPtr]::Zero) { break }
                        Start-Sleep -Milliseconds 200
                    }
                    if ($target -eq [IntPtr]::Zero) {
                        # Leave a picture of what was there instead: a step that
                        # opens no window is otherwise invisible.
                        Save-Shot ([Win32Capture]::VisibleRect($hwnd)) $Out
                        throw "no window titled ""$wanted"" to click in; see $Out"
                    }
                    if (-not [Win32Capture]::BringToFront($target)) {
                        Write-Warning "$wanted would not come to the front"
                    }
                    $currentTitle = $wanted
                    $currentHwnd = $target
                    $rect = if ($target -eq $hwnd) {
                        [Win32Capture]::VisibleRect($target)
                    } else {
                        [Win32Capture]::ClientRect($target)
                    }
                    Write-Host ("window: $wanted at {0},{1} {2}x{3}" -f
                        $rect.Left, $rect.Top,
                        ($rect.Right - $rect.Left), ($rect.Bottom - $rect.Top))
                }
                'remove' {
                    if (Test-Path -LiteralPath $value) {
                        Remove-Item -LiteralPath $value -Force
                    }
                }
                'shot' {
                    # A picture of whichever window the steps are addressing,
                    # for working out why one of them landed nowhere.
                    Save-Shot $rect $value
                    Write-Host "shot:   $value"
                }
                'film' {
                    # Start filming the current window, and keep filming while
                    # the steps that follow run, until `endfilm`. Opening and
                    # closing a window is when it is most likely to flicker,
                    # and a recording that starts after the fact misses it.
                    if ($film) { throw 'already filming' }
                    $ffmpeg = Get-Ffmpeg
                    $filmTo = $value
                    $filmVideo = "$value.mkv"
                    Remove-Item -LiteralPath $filmVideo -Force -ErrorAction SilentlyContinue
                    # Always the window under test, whatever the steps are
                    # addressing: anything it opens is on top of it, so one
                    # region catches the lot. Addressing a window brings it to
                    # the front, and doing that to the window behind a modal
                    # would stage the very fight this is here to catch.
                    $area = [Win32Capture]::VisibleRect($hwnd)
                    $fw = $area.Right - $area.Left
                    $fh = $area.Bottom - $area.Top
                    $psi = New-Object System.Diagnostics.ProcessStartInfo
                    $psi.FileName = $ffmpeg
                    $psi.Arguments = (
                        '-hide_banner -loglevel error -f gdigrab -framerate 60 ' +
                        "-offset_x $($area.Left) -offset_y $($area.Top) " +
                        "-video_size ${fw}x${fh} -i desktop -c:v ffv1 ""$filmVideo""")
                    $psi.RedirectStandardInput = $true
                    $psi.RedirectStandardError = $true
                    $psi.UseShellExecute = $false
                    $psi.CreateNoWindow = $true
                    $film = [System.Diagnostics.Process]::Start($psi)
                    Start-Sleep -Milliseconds 400
                }
                'wiggle' {
                    # Drag the pointer across the window and off it. Flicker
                    # that depends on where the cursor is needs the cursor to
                    # go there.
                    $w = $rect.Right - $rect.Left
                    $h = $rect.Bottom - $rect.Top
                    $path = @(
                        @(0.5, 0.5), @(0.2, 0.2), @(0.8, 0.9), @(0.5, 0.1),
                        @(1.6, 0.5), @(0.5, 2.5), @(-0.6, 0.5), @(0.5, -1.5)
                    )
                    $until = [DateTime]::UtcNow.AddMilliseconds([int]$value)
                    $step = 0
                    while ([DateTime]::UtcNow -lt $until) {
                        $at = $path[$step % $path.Count]
                        [Win32Capture]::SetCursorPos(
                            $rect.Left + [int]($w * $at[0]),
                            $rect.Top + [int]($h * $at[1])) | Out-Null
                        Start-Sleep -Milliseconds 100
                        $step++
                    }
                }
                'endfilm' {
                    if (-not $film) { throw 'not filming' }
                    # `q` is how ffmpeg is asked to stop and close the file
                    # properly; killing it leaves nothing readable behind.
                    $film.StandardInput.WriteLine('q')
                    if (-not $film.WaitForExit(15000)) {
                        $film.Kill()
                        throw 'the recording would not finish'
                    }
                    $said = $film.StandardError.ReadToEnd()
                    $film = $null
                    if (-not (Test-Path $filmVideo)) {
                        throw "nothing was recorded: $said"
                    }
                    # How many times the picture actually changed, by throwing
                    # away every frame identical to the one before it.
                    #
                    # Not scene detection: a title bar going from active to
                    # inactive and back is a handful of pixels, far under any
                    # scene threshold, and is exactly the flicker worth
                    # catching. A window that is settling draws a few distinct
                    # frames; one that is flickering draws dozens.
                    $total = & $countFilmFrames "$filmTo.frames.txt" $null $filmVideo
                    $distinct = & $countFilmFrames "$filmTo.distinct.txt" `
                        'mpdecimate' $filmVideo
                    [System.IO.File]::WriteAllText(
                        "$filmTo.txt", "frames=$total`ndistinct_frames=$distinct`n")
                    Write-Host "film:   $distinct distinct of $total frames; $filmVideo"
                }
                default { throw "unknown step: $step" }
            }
        }
    }

    if ($HoverAt) {
        $parts = $HoverAt -split ','
        $x = $rect.Left + [int]$parts[0]
        $y = $rect.Top + [int]$parts[1]
        [Win32Capture]::SetCursorPos($x, $y) | Out-Null
        # The window redraws on the enter event, not before it.
        Start-Sleep -Milliseconds 700
    }

    # The picture is always of the window under test, whichever window the
    # steps were last addressing.
    $rect = [Win32Capture]::VisibleRect($hwnd)
    if ($rect.Right -le $rect.Left) { $rect = $hostRect }
    $width = $rect.Right - $rect.Left
    $height = $rect.Bottom - $rect.Top

    $full = if ([System.IO.Path]::IsPathRooted($Out)) {
        $Out
    } else {
        Join-Path (Get-Location) $Out
    }
    Save-Shot $rect $full

    Write-Output "captured ${width}x${height} -> $Out"
}
finally {
    if (-not $proc.HasExited) {
        if ($CloseCleanly -and $hwnd -ne [IntPtr]::Zero) {
            [Win32Capture]::PostMessage($hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
            if (-not $proc.WaitForExit(5000)) { Stop-Process -Id $proc.Id -Force }
        } else {
            Stop-Process -Id $proc.Id -Force
        }
    }
}
