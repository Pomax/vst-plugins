<#
.SYNOPSIS
Screenshot the real editor window.

.DESCRIPTION
The headless pixel tests prove the drawing code is right, but they do not prove
the actual window is: that path goes through baseview and OpenGL, and the
background is the renderer's clear colour rather than anything egui draws. This
launches the given program, waits for its window to appear, captures its pixels
off the screen with BitBlt, and closes it.

.EXAMPLE
powershell -ExecutionPolicy Bypass -File tools/capture-window.ps1 -Exe binaries/mini-host.exe -Title "Mini VST Host" -Out window.png
#>
param(
    [ValidateSet('light', 'dark', 'auto')]
    [string]$Theme = 'auto',
    [string]$Out = 'window.png',
    [string]$Exe = 'binaries/mini-host.exe',
    [string]$Notes = '',
    # Arguments to launch with. Given, they replace -Theme and -Notes, for an
    # executable that does not take those. Separate them with `|`: with
    # `powershell -File`, only one token binds to a parameter.
    [string]$ExeArgs = '',
    [string]$Title = 'Markdown Notes',
    # A sequence of actions, run in order before the grab:
    #   click:X,Y     left click at X,Y inside the window
    #   type:TEXT     send TEXT as key presses, one at a time
    #   remove:PATH   delete a file, so a save does not hit "already exists"
    #   geometry:PATH write down what the window and the plugin inside it measure
    #   hold:X,Y      press the button there and keep holding it
    #   moveto:X,Y    move the pointer there, at the speed a hand moves
    #   press:LABEL|X,Y  photograph the window, find the labelled control
    #                 nearest X,Y in the picture to confirm or correct the spot,
    #                 delete the picture, and click where the label really is
    #   showing:TEXT|Y   fail unless the window below Y shows TEXT
    #   hidden:TEXT|Y    fail if the window below Y shows TEXT
    #   dialog:WORD   fail unless a file dialog with WORD on it is open
    #   nodialog:WORD fail if a file dialog is open at all
    #   nowindow:TITLE fail while a window with that title is still there
    #   letgo:X,Y     move there and release the button
    [string[]]$Steps = @(),
    # The same, one per line, from a file. `powershell -File` cannot bind more
    # than one token to an array parameter, so a sequence comes from here.
    [string]$StepFile = '',
    # Close the window by asking it to, rather than killing the process, so
    # the program runs its shutdown. A UI test needs this: the state it
    # asserts on is written on the way out.
    [switch]$CloseCleanly
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
    [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
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
            // Not in front yet: look again shortly.
            System.Threading.Thread.Sleep(25);
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

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern short VkKeyScanW(char c);

    /// Type one character the way a keyboard does: the key it is on, with
    /// Shift held for exactly as long as that key needs it. The shift is this
    /// call's own, pressed and released here, so the case that arrives is the
    /// case that was written and not whatever the keyboard was left in.
    public static void TypeChar(char c) {
        short found = VkKeyScanW(c);
        if (found == -1) return;
        ushort vk = (ushort)(found & 0xff);
        bool shifted = (found & 0x100) != 0;
        if (shifted) Send(KeyInput(0x10, false));   // Shift down
        Send(KeyInput(vk, false));
        Send(KeyInput(vk, true));
        if (shifted) Send(KeyInput(0x10, true));    // Shift up
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
    // What a window on Windows shows while something is being dragged, and so
    // what baseview asks for when egui says the pointer is grabbing.
    const int IDC_SIZEALL = 32646;

    /// The name of the cursor currently on screen, as far as the stock set goes.
    public static string CursorNow() {
        var info = new CURSORINFO();
        info.cbSize = Marshal.SizeOf(typeof(CURSORINFO));
        if (!GetCursorInfo(ref info)) return "unreadable";
        if (info.hCursor == LoadCursorW(IntPtr.Zero, IDC_IBEAM)) return "ibeam";
        if (info.hCursor == LoadCursorW(IntPtr.Zero, IDC_ARROW)) return "arrow";
        if (info.hCursor == LoadCursorW(IntPtr.Zero, IDC_HAND)) return "hand";
        if (info.hCursor == LoadCursorW(IntPtr.Zero, IDC_SIZEALL)) return "grabbing";
        return "other";
    }

    /// Hold a modifier, tap a key, let go, as a keyboard does it.
    public static void Shortcut(ushort modifier, ushort key) {
        Send(KeyInput(modifier, false));
        System.Threading.Thread.Sleep(KEY_MS);
        Send(KeyInput(key, false));
        System.Threading.Thread.Sleep(KEY_MS);
        Send(KeyInput(key, true));
        System.Threading.Thread.Sleep(KEY_MS);
        Send(KeyInput(modifier, true));
    }

    /// How long a hand takes between one key of a shortcut and the next.
    const int KEY_MS = 30;

    static void Send(INPUT input) {
        var one = new INPUT[] { input };
        SendInput(1, one, Marshal.SizeOf(typeof(INPUT)));
    }

    /// How long a finger holds a button down for a click. Sent back to back,
    /// the press and the release can land in one frame and be missed.
    const int HELD_MS = 60;

    /// A click the UI can see: the pointer arrives, presses, and releases.
    public static void Click(int x, int y) {
        SetCursorPos(x, y);
        System.Threading.Thread.Sleep(HELD_MS);
        mouse_event(0x0002, 0, 0, 0, System.IntPtr.Zero); // left down
        System.Threading.Thread.Sleep(HELD_MS);
        mouse_event(0x0004, 0, 0, 0, System.IntPtr.Zero); // left up
    }
    /// Press at one point, move across, release at another: a drag.
    ///
    /// The pointer moves in steps rather than jumping, because a UI works out
    /// what is being selected from where the pointer goes, not from where it
    /// ends up.
    /// Move the pointer there the way a hand does: across, not by teleporting.
    ///
    /// A window being dragged sees where the pointer is, over and over. Put it
    /// somewhere in one jump and the window is asked to go from one size
    /// straight to another, which is nothing like what happens when a person
    /// drags a corner and is no test of whether the drawing keeps up.
    public static void Glide(int fromX, int fromY, int toX, int toY, int overMs) {
        int steps = System.Math.Max(1, overMs / 16);
        for (int i = 1; i <= steps; i++) {
            SetCursorPos(
                fromX + (toX - fromX) * i / steps,
                fromY + (toY - fromY) * i / steps);
            System.Threading.Thread.Sleep(16);
        }
    }

    /// Press the button where the pointer is, having glided there first.
    ///
    /// Held down until `LetGo`, so a test can look at what the window draws
    /// while something is being dragged. A drag that presses and releases in
    /// one call can only ever be judged by what it left behind.
    public static void TakeHold(int x, int y) {
        POINT from;
        GetCursorPos(out from);
        Glide(from.X, from.Y, x, y, 100);
        mouse_event(0x0002, 0, 0, 0, System.IntPtr.Zero); // left down
    }

    /// Move the pointer to a place, across the screen rather than in one jump,
    /// so the window under it sees the pointer arrive.
    public static void MoveTo(int x, int y) {
        POINT from;
        GetCursorPos(out from);
        Glide(from.X, from.Y, x, y, 100);
    }

    public static void LetGo(int x, int y) {
        MoveTo(x, y);
        mouse_event(0x0004, 0, 0, 0, System.IntPtr.Zero); // left up
    }

    /// Drag a window's bottom right corner over `overMs`, and let go.
    public static void DragCorner(int x, int y, int dx, int dy, int overMs) {
        POINT from;
        GetCursorPos(out from);
        // From wherever the pointer is now, not from thin air.
        Glide(from.X, from.Y, x, y, 100);
        mouse_event(0x0002, 0, 0, 0, System.IntPtr.Zero); // left down
        Glide(x, y, x + dx, y + dy, overMs);
        mouse_event(0x0004, 0, 0, 0, System.IntPtr.Zero); // left up
    }

    public static void Drag(int fromX, int fromY, int toX, int toY) {
        SetCursorPos(fromX, fromY);
        System.Threading.Thread.Sleep(HELD_MS);
        mouse_event(0x0002, 0, 0, 0, System.IntPtr.Zero); // left down
        System.Threading.Thread.Sleep(HELD_MS);
        Glide(fromX, fromY, toX, toY, 130);
        System.Threading.Thread.Sleep(HELD_MS);
        mouse_event(0x0004, 0, 0, 0, System.IntPtr.Zero); // left up
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

    /// The window rect the window manager knows, invisible resize border and
    /// all. That border is where a corner can be grabbed, and it is outside the
    /// painted bounds, so a drag has to aim at this rect and not at those.
    public static RECT OuterRect(IntPtr hWnd) {
        RECT r;
        GetWindowRect(hWnd, out r);
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

    [DllImport("user32.dll")] public static extern bool SetWindowPos(
        IntPtr hWnd, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr hWnd, uint cmd);

    /// The plugin's own window inside the host's: the biggest child there is.
    ///
    /// A plugin may own more than one window, and which is first in Z-order is
    /// not something to rely on, so the one that fills the frame wins.
    public static IntPtr EditorWindow(IntPtr parent) {
        IntPtr best = IntPtr.Zero;
        long biggest = -1;
        IntPtr child = GetWindow(parent, 5);  // GW_CHILD
        while (child != IntPtr.Zero) {
            RECT r;
            if (GetWindowRect(child, out r)) {
                long area = (long)(r.Right - r.Left) * (r.Bottom - r.Top);
                if (area > biggest) { biggest = area; best = child; }
            }
            child = GetWindow(child, 2);  // GW_HWNDNEXT
        }
        return best;
    }

    /// Where the plugin's window sits inside the host's drawable area.
    public static RECT EditorInHost(IntPtr parent) {
        IntPtr editor = EditorWindow(parent);
        RECT r; GetWindowRect(editor, out r);
        POINT origin; origin.X = 0; origin.Y = 0;
        ClientToScreen(parent, ref origin);
        RECT inside;
        inside.Left = r.Left - origin.X;
        inside.Top = r.Top - origin.Y;
        inside.Right = r.Right - origin.X;
        inside.Bottom = r.Bottom - origin.Y;
        return inside;
    }

    public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lParam);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    // CharSet.Unicode matters: without it the wide title marshals as ANSI and
    // comes back as just its first character.
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetWindowTextW(IntPtr hWnd, StringBuilder text, int count);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetClassNameW(IntPtr hWnd, StringBuilder name, int count);



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

    /// A visible window of this process that is not `notThis`.
    ///
    /// A file dialog is a window of the process that opened it, so a second
    /// window being there is a dialog being up. Zero when there is none.
    ///
    /// A program started from a console has that console as a visible window
    /// of its own for as long as it runs, and that is not a dialog.
    public static IntPtr OtherWindow(uint targetPid, IntPtr notThis) {
        IntPtr found = IntPtr.Zero;
        long biggestArea = 0;
        EnumWindows((h, l) => {
            uint pid;
            GetWindowThreadProcessId(h, out pid);
            if (pid != targetPid || h == notThis || !IsWindowVisible(h)) return true;
            var kind = new StringBuilder(64);
            GetClassNameW(h, kind, 64);
            if (kind.ToString() == "ConsoleWindowClass") return true;
            RECT r;
            if (!GetWindowRect(h, out r)) return true;
            // Nor is a helper window a few pixels across, which a program
            // may keep for its own purposes: a dialog has room for a button.
            if (r.Right - r.Left < 150 || r.Bottom - r.Top < 80) return true;
            long area = (long)(r.Right - r.Left) * (r.Bottom - r.Top);
            if (area > biggestArea) { biggestArea = area; found = h; }
            return true;
        }, IntPtr.Zero);
        return found;
    }
}
'@

[Win32Capture]::SetProcessDPIAware() | Out-Null

if (-not (Test-Path $Exe)) {
    throw "$Exe not found - run build.bat"
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

# What is actually handed to the program. `Start-Process` joins its list with
# spaces and quotes nothing, so a path with a space in it arrives as two
# arguments and the program is launched with a file name that does not exist.
# The unquoted list is kept as well: the restart step reads the state path out
# of it.
$launchLine = ($launchArgs | ForEach-Object { '"{0}"' -f $_ }) -join ' '

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

# Every piece of text in a picture, with where each one is.
#
# The whole picture is read once and the answer searched afterwards. Asking the
# recogniser for one string is not what it does: it reads what is there either
# way, and a small picture cut to one control is read far worse than a whole
# window.
function Read-Text([string]$path) {
    $finder = Join-Path $PSScriptRoot '..\binaries\find-text.exe'
    if (-not (Test-Path $finder)) {
        throw "the text finder is not built: run tools\find-text\build.bat"
    }
    $answer = @(& $finder $path)
    if ($LASTEXITCODE -ne 0) { throw "the text finder failed on $path" }

    $lines = @()
    foreach ($row in $answer | Select-Object -Skip 1) {
        $parts = $row -split '\s+', 5
        if ($parts.Count -ne 5) { continue }
        $lines += @{
            Text = $parts[4]
            Left = [int]$parts[0]; Top = [int]$parts[1]
            Right = [int]$parts[0] + [int]$parts[2]
            Bottom = [int]$parts[1] + [int]$parts[3]
            X = [int]$parts[0] + [int]([int]$parts[2] / 2)
            Y = [int]$parts[1] + [int]([int]$parts[3] / 2)
        }
    }
    return $lines
}

# Every place a piece of text is in a picture, best match first.
#
# The box is of the words that match, not of the whole line they were read on:
# neighbouring buttons come back as one line, and the centre of that line is
# the gap between them rather than either button.
function Find-All([string]$path, [string]$label) {
    $finder = Join-Path $PSScriptRoot '..\binaries\find-text.exe'
    if (-not (Test-Path $finder)) {
        throw "the text finder is not built: run tools\find-text\build.bat"
    }
    $answer = @(& $finder $path $label)
    if ($LASTEXITCODE -eq 1) { return @() }
    if ($LASTEXITCODE -ne 0) { throw "the text finder failed on $path" }

    $found = @()
    foreach ($row in $answer | Select-Object -Skip 1) {
        $parts = $row -split '\s+'
        if ($parts.Count -ne 4) { continue }
        $found += @{
            Left = [int]$parts[0]; Top = [int]$parts[1]
            Right = [int]$parts[0] + [int]$parts[2]
            Bottom = [int]$parts[1] + [int]$parts[3]
            X = [int]$parts[0] + [int]([int]$parts[2] / 2)
            Y = [int]$parts[1] + [int]([int]$parts[3] / 2)
        }
    }
    return $found
}

# Where a labelled control is inside a picture, found by reading it.
#
# With `$near` given, the match closest to that spot wins, so a word that is
# both a tab and a heading is told apart by where it is. Returns the centre as
# a hashtable with X and Y in picture pixels, or $null when the label is
# nowhere in the picture.
function Find-Label([string]$path, [string]$label, $near) {
    $found = @(Find-All $path $label)
    if (-not $found.Count) { return $null }
    if (-not $near) { return $found[0] }
    return $found | Sort-Object {
        [Math]::Pow($_.X - $near.X, 2) + [Math]::Pow($_.Y - $near.Y, 2)
    } | Select-Object -First 1
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

# How long a look that found nothing is left before looking again.
$POLL_MS = 25

# Look for something until it is there, and hand it back, or hand back nothing
# once TimeoutMs has gone by without it.
#
# This is the only waiting the driver does. No step pauses for a length of
# time: a step that needs the window to have caught up says what it is waiting
# to see, and carries on the moment it is there.
function Wait-Until([scriptblock]$Look, [int]$TimeoutMs = 5000) {
    $deadline = [DateTime]::UtcNow.AddMilliseconds($TimeoutMs)
    while ($true) {
        $found = & $Look
        if ($found) { return $found }
        if ([DateTime]::UtcNow -ge $deadline) { return $null }
        Start-Sleep -Milliseconds $POLL_MS
    }
}

# Whether a part of the screen has something drawn in it: a window that has
# not drawn its first frame yet is one flat colour, and one that has is not.
# A grid of points is looked at rather than every pixel.
function Test-Drawn([Win32Capture+RECT]$area) {
    Add-Type -AssemblyName System.Drawing
    $w = $area.Right - $area.Left
    $h = $area.Bottom - $area.Top
    if ($w -le 0 -or $h -le 0) { return $false }
    $bmp = New-Object System.Drawing.Bitmap $w, $h
    $gfx = [System.Drawing.Graphics]::FromImage($bmp)
    try {
        $gfx.CopyFromScreen($area.Left, $area.Top, 0, 0,
            (New-Object System.Drawing.Size $w, $h))
        $seen = @{}
        for ($x = 2; $x -lt $w; $x += [Math]::Max(1, [int]($w / 40))) {
            for ($y = 2; $y -lt $h; $y += [Math]::Max(1, [int]($h / 20))) {
                $seen[$bmp.GetPixel($x, $y).ToArgb()] = $true
            }
        }
        return $seen.Count -ge 3
    } finally {
        $gfx.Dispose()
        $bmp.Dispose()
    }
}

# Wait for a window to have drawn itself: one that has just opened is blank
# until its first frame, and there is nothing in a blank window to click or
# type into.
function Wait-Drawn([Win32Capture+RECT]$area) {
    $drawn = Wait-Until { Test-Drawn $area } 15000
    if (-not $drawn) { throw 'the window opened and never drew anything' }
}

# Wait for the plugin inside the host's window to be up, the way a person
# does before touching it: until there is something to read below the host's
# own strip, which is the plugin's toolbar. The host draws its strip and its
# background long before the plugin has loaded and drawn its first frame, so
# the window having colour in it says nothing about the plugin.
function Wait-Loaded([Win32Capture+RECT]$area) {
    $probe = if ($outDir) {
        Join-Path $outDir 'capture-window-loaded.png'
    } else {
        Join-Path $env:TEMP 'capture-window-loaded.png'
    }
    $loaded = Wait-Until {
        Save-Shot $area $probe
        try {
            @(Read-Text $probe | Where-Object { $_.Top -ge 40 }).Count -gt 0
        } catch { $false }
    } 30000
    Remove-Item -LiteralPath $probe -Force -ErrorAction SilentlyContinue
    if (-not $loaded) { throw 'the plugin never drew anything to read in the host window' }
    Start-Sleep -Milliseconds $HAND_MS
}

# How long a hand takes over one key, and how long it takes to go from the
# mouse to the next thing. Input sent faster than a person can make it reaches
# the plugin out of order: the pointer and the keyboard come in by different
# roads, and keys sent in the same instant as a click overtake it.
$KEY_MS = 12
$HAND_MS = 150

# Type what a `type:` step says, one key at a time. The text is in SendKeys
# notation, where `{...}` is one named key (or one key repeated) and anything
# else is a character of its own. A character goes as its key, with a shift
# of its own when it needs one, so the case it arrives in is the case it was
# written; a named key has no character and goes through SendKeys.
function Send-Typing([string]$keys) {
    foreach ($key in [regex]::Matches($keys, '\{[^}]*\}|.')) {
        if ($key.Value.StartsWith('{')) {
            [System.Windows.Forms.SendKeys]::SendWait($key.Value)
        } else {
            [Win32Capture]::TypeChar([char]$key.Value)
        }
        Start-Sleep -Milliseconds $KEY_MS
    }
}

$proc = Start-Process -FilePath $Exe -ArgumentList $launchLine -PassThru

try {
    # Wait for the editor window to exist.
    $hwnd = Wait-Until {
        $seen = [Win32Capture]::FindWindow([uint32]$proc.Id, $Title, $true)
        if ($seen -ne [IntPtr]::Zero) { $seen }
    } 30000
    if (-not $hwnd) { $hwnd = [IntPtr]::Zero }
    if ($hwnd -eq [IntPtr]::Zero) {
        # No window with that title turned up; take the largest one and say so.
        $hwnd = [Win32Capture]::FindWindow([uint32]$proc.Id, $Title, $false)
        Write-Warning "no window titled ""$Title"" found; falling back to the largest one"
    }
    if ($hwnd -eq [IntPtr]::Zero) { throw 'the window never appeared' }
    [Win32Capture]::ShowWindow($hwnd, 5) | Out-Null   # SW_SHOW
    if (-not [Win32Capture]::BringToFront($hwnd)) {
        Write-Warning 'the window would not come to the front'
    }

    $rect = [Win32Capture]::VisibleRect($hwnd)
    Wait-Loaded $rect
    # The screenshot is always of the window under test, whichever window the
    # steps were last clicking in.
    $hostRect = $rect
    # Which window the steps are addressing, for recording it.
    $currentTitle = $Title
    $currentHwnd = $hwnd

    if ($StepFile) {
        $Steps = Get-Content -Path $StepFile |
            Where-Object { $_.Trim() -ne '' -and -not $_.StartsWith('#') }
    }

    if ($Steps.Count -gt 0) {
        Add-Type -AssemblyName System.Windows.Forms
        $restarts = 0
        foreach ($step in $Steps) {
            $kind, $value = $step -split ':', 2
            switch ($kind) {
                'click' {
                    $parts = $value -split ','
                    [Win32Capture]::Click(
                        $rect.Left + [int]$parts[0], $rect.Top + [int]$parts[1])
                    Start-Sleep -Milliseconds $HAND_MS
                }
                'press' {
                    # `press:LABEL|X,Y` looks first and clicks second. X,Y is
                    # where the code puts the control, in window coordinates;
                    # the window is photographed, the label nearest that spot
                    # is found in the picture, and the click goes where the
                    # label really is. A label the window does not show is a
                    # failure, not a blind click.
                    $label, $at = $value -split '\|', 2
                    $label = $label.Trim()
                    if (-not $label -or -not $at) { throw "cannot read press: $value" }
                    $parts = $at -split ','
                    if ($parts.Count -ne 2) { throw "cannot read press: $value" }
                    $x = [int]$parts[0]
                    $y = [int]$parts[1]
                    $probe = if ($outDir) {
                        Join-Path $outDir 'capture-window-press.png'
                    } else {
                        Join-Path $env:TEMP 'capture-window-press.png'
                    }
                    # A pointer left on the control by the run before draws it
                    # highlighted, and a window that opened under a pointer
                    # that never moves has not been told where the pointer is.
                    # It is put on the title bar first, in one jump, and travels
                    # to the control from there.
                    if ($currentHwnd -eq $hwnd) {
                        [Win32Capture]::SetCursorPos(
                            $rect.Left + [int](($rect.Right - $rect.Left) / 2),
                            $rect.Top + 12) | Out-Null
                    }
                    # Looked for until it is there: the control may be one the
                    # step before has only just caused to be drawn.
                    $found = Wait-Until {
                        Save-Shot $rect $probe
                        Find-Label $probe $label @{ X = $x; Y = $y }
                    }
                    if (-not $found) {
                        # The picture stays when the label is not in it, and
                        # what was read in it is said: what the window actually
                        # held is the only way to tell a control drawn
                        # elsewhere from one that cannot be read.
                        $saw = (Read-Text $probe | ForEach-Object { $_.Text }) -join ' | '
                        throw "no control labelled `"$label`" is in the window; it reads: $saw; it is in $probe"
                    }
                    Remove-Item -LiteralPath $probe -Force -ErrorAction SilentlyContinue
                    [Win32Capture]::MoveTo(
                        $rect.Left + [int]$found.X, $rect.Top + [int]$found.Y)
                    [Win32Capture]::Click(
                        $rect.Left + [int]$found.X, $rect.Top + [int]$found.Y)
                    Start-Sleep -Milliseconds $HAND_MS
                    Write-Host ("press:  {0} at {1},{2}" -f $label, $found.X, $found.Y)
                }
                'type' {
                    Send-Typing $value
                }
                { $_ -in 'showing', 'hidden' } {
                    # `showing:TEXT|Y` reads the window below Y and fails
                    # unless TEXT is there; `hidden:` fails if it is. Below Y
                    # so the toolbars and the tab labels are out of it: what
                    # is being asked about is what the document displays, and
                    # a tab's own label is not that.
                    $text, $below = $value -split '\|', 2
                    $text = $text.Trim()
                    if (-not $text -or -not $below) { throw "cannot read ${kind}: $value" }
                    # The whole window is photographed and the answer filtered
                    # to what is below Y afterwards: a picture cut to a band is
                    # read worse than a whole window.
                    $probe = if ($outDir) {
                        Join-Path $outDir 'capture-window-showing.png'
                    } else {
                        Join-Path $env:TEMP 'capture-window-showing.png'
                    }
                    try {
                        # Looked at until it is as the step says: what the step
                        # before did may not have been drawn yet, and a tooltip
                        # takes its time coming and going.
                        $asSaid = Wait-Until {
                            Save-Shot $rect $probe
                            $there = @(Find-All $probe $text |
                                Where-Object { $_.Top -ge [int]$below }).Count -gt 0
                            $there -eq ($kind -eq 'showing')
                        }
                        $found = if ($asSaid) { $kind -eq 'showing' } else { $kind -ne 'showing' }
                        # Only a step that is about to fail reads the window
                        # again, to say what it holds instead.
                        $saw = ''
                        if (-not $asSaid) {
                            $saw = (Read-Text $probe |
                                Where-Object { $_.Top -ge [int]$below } |
                                ForEach-Object { $_.Text }) -join ' | '
                        }
                    } finally {
                        Remove-Item -LiteralPath $probe -Force -ErrorAction SilentlyContinue
                    }
                    if ($kind -eq 'showing' -and -not $found) {
                        throw "the document does not show `"$text`"; below $below the window reads: $saw"
                    }
                    if ($kind -eq 'hidden' -and $found) {
                        throw "the document still shows `"$text`"; below $below the window reads: $saw"
                    }
                    Write-Host ("{0}: {1}" -f $kind, $text)
                }
                { $_ -in 'dialog', 'nodialog' } {
                    # `dialog:WORD` fails unless a file dialog is open with
                    # WORD on it; `nodialog:WORD` fails if one is open at all.
                    #
                    # A file dialog is a window of the process that opened it,
                    # so a second window of that process being there is a
                    # dialog being up. Which dialog is read off the dialog
                    # itself: the title is the plugin's name on both of them,
                    # and only what is written inside says which.
                    $want = $value.Trim()
                    if ($kind -eq 'nodialog') {
                        $closed = Wait-Until {
                            [Win32Capture]::OtherWindow([uint32]$proc.Id, $hwnd) -eq [IntPtr]::Zero
                        } 3000
                        if (-not $closed) { throw "a $want dialog is still open" }
                        Write-Host ("nodialog: {0}" -f $want)
                        break
                    }
                    $panel = Wait-Until {
                        $seen = [Win32Capture]::OtherWindow([uint32]$proc.Id, $hwnd)
                        if ($seen -ne [IntPtr]::Zero) { $seen }
                    } 10000
                    if (-not $panel) {
                        throw "no $want dialog is open, so the button did nothing"
                    }
                    $probe = if ($outDir) {
                        Join-Path $outDir 'capture-window-dialog.png'
                    } else {
                        Join-Path $env:TEMP 'capture-window-dialog.png'
                    }
                    # The window is looked up again on every look: the first
                    # second window to turn up need not be the dialog itself.
                    $named = Wait-Until {
                        $seen = [Win32Capture]::OtherWindow([uint32]$proc.Id, $hwnd)
                        if ($seen -ne [IntPtr]::Zero) {
                            Save-Shot ([Win32Capture]::VisibleRect($seen)) $probe
                            Find-Label $probe $want
                        }
                    } 10000
                    if (-not $named) {
                        throw "a dialog is open, but nothing on it says ""$want""; see $probe"
                    }
                    Remove-Item -LiteralPath $probe -Force -ErrorAction SilentlyContinue
                    Start-Sleep -Milliseconds $HAND_MS
                    Write-Host ("dialog: {0}" -f $want)
                }
                'nowindow' {
                    # `nowindow:TITLE` fails while a window with that title is
                    # still there: what a dialog that closed cleanly leaves.
                    $want = $value.Trim()
                    $gone = Wait-Until {
                        [Win32Capture]::FindWindow([uint32]$proc.Id, $want, $true) -eq [IntPtr]::Zero
                    } 3000
                    if (-not $gone) { throw "the window ""$want"" is still open" }
                    Write-Host ("nowindow: {0}" -f $want)
                }
                'dragtext' {
                    # `dragtext:TEXT|Y|X,Y2` selects by dragging from where
                    # TEXT starts to X,Y2 in window coordinates. Where TEXT is
                    # comes from looking: the window below Y is photographed,
                    # the first TEXT in it is found, the picture is deleted,
                    # and the press lands on TEXT's first character.
                    $text, $below, $to = $value -split '\|', 3
                    if (-not $text -or -not $below -or -not $to) {
                        throw "cannot read dragtext: $value"
                    }
                    $parts = $to -split ','
                    if ($parts.Count -ne 2) { throw "cannot read dragtext: $value" }
                    $probe = Join-Path $env:TEMP 'capture-window-press.png'
                    # Looked for until it is there: the text may be what the
                    # step before typed, and not drawn yet.
                    $found = Wait-Until {
                        Save-Shot $rect $probe
                        @(Find-All $probe $text.Trim() |
                            Where-Object { $_.Top -ge [int]$below })[0]
                    }
                    if (-not $found) {
                        $read = @(Read-Text $probe | Where-Object { $_.Top -ge [int]$below })
                        # What was readable instead: a step that says only "not
                        # found" cannot tell a word drawn elsewhere from a word
                        # the reader could not make out.
                        $saw = ($read | ForEach-Object { $_.Text }) -join ' | '
                        throw "`"$text`" is not on screen to select from; below $below the window reads: $saw; see $probe"
                    }
                    Remove-Item -LiteralPath $probe -Force -ErrorAction SilentlyContinue
                    # On the first glyph, not beside it: a press lands on the
                    # nearest character boundary, and the boundary before the
                    # first letter is the one its ink starts at.
                    [Win32Capture]::Drag(
                        $rect.Left + [int]$found.Left,
                        $rect.Top + [int](($found.Top + $found.Bottom) / 2),
                        $rect.Left + [int]$parts[0], $rect.Top + [int]$parts[1])
                    Start-Sleep -Milliseconds $HAND_MS
                    Write-Host ("dragtext: {0}" -f $text.Trim())
                }
                'hold' {
                    # `hold:X,Y` presses and keeps holding, so the steps after
                    # it happen mid-drag.
                    $parts = $value -split ','
                    if ($parts.Count -ne 2) { throw "cannot read hold: $value" }
                    [Win32Capture]::TakeHold(
                        $rect.Left + [int]$parts[0], $rect.Top + [int]$parts[1])
                }
                'moveto' {
                    # `moveto:X,Y` moves the pointer there, button or no button.
                    $parts = $value -split ','
                    if ($parts.Count -ne 2) { throw "cannot read moveto: $value" }
                    [Win32Capture]::MoveTo(
                        $rect.Left + [int]$parts[0], $rect.Top + [int]$parts[1])
                }
                'letgo' {
                    # `letgo:X,Y` moves there and releases the button.
                    $parts = $value -split ','
                    if ($parts.Count -ne 2) { throw "cannot read letgo: $value" }
                    [Win32Capture]::LetGo(
                        $rect.Left + [int]$parts[0], $rect.Top + [int]$parts[1])
                    Start-Sleep -Milliseconds $HAND_MS
                }
                'cursor' {
                    # `cursor:X,Y|ibeam` parks the pointer and checks what the
                    # window asked the cursor to be there.
                    $where, $want = $value -split '\|', 2
                    $parts = $where -split ','
                    [Win32Capture]::SetCursorPos(
                        $rect.Left + [int]$parts[0], $rect.Top + [int]$parts[1]) | Out-Null
                    # The window only changes it when it next redraws, so it is
                    # looked at until it is what was asked for.
                    $right = Wait-Until { [Win32Capture]::CursorNow() -eq $want.Trim() } 2000
                    if (-not $right) {
                        $shown = [Win32Capture]::CursorNow()
                        throw "cursor at $where is $shown, expected $($want.Trim())"
                    }
                }
                'shortcut' {
                    # `shortcut:ctrl+a` is a modifier held down over one key.
                    $parts = $value.Trim().ToLower() -split '\+'
                    if ($parts.Count -ne 2) { throw "cannot read shortcut: $value" }
                    $modifier = switch ($parts[0]) {
                        'ctrl'  { 0x11 }
                        'shift' { 0x10 }
                        'alt'   { 0x12 }
                        default { throw "unknown modifier: $($parts[0])" }
                    }
                    $letter = $parts[1]
                    if ($letter.Length -ne 1) { throw "unknown key: $letter" }
                    $key = [uint16][char]($letter.ToUpper())
                    [Win32Capture]::Shortcut([uint16]$modifier, $key)
                }
                'kill' {
                    # Force quit, for a test that ends with a native modal
                    # dialog up: nothing can ask the window to close
                    # underneath one. The program gets no shutdown, so such a
                    # test asserts on files, not state.
                    Stop-Process -Id $proc.Id -Force
                    $proc.WaitForExit(5000) | Out-Null
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
                    # Put aside what the run that just ended wrote, as STATE.1
                    # for the first run, STATE.2 for the second: the runner
                    # checks it against the expectations written before this
                    # step. It is moved rather than left, so what is asserted
                    # afterwards can only have come from the new run. Otherwise
                    # a second run that dies quietly leaves the first run's
                    # state behind and the test passes on it.
                    $restarts++
                    for ($i = 0; $i -lt $launchArgs.Count - 1; $i++) {
                        if ($launchArgs[$i] -eq '--state') {
                            $written = $launchArgs[$i + 1]
                            if (Test-Path -LiteralPath $written) {
                                Move-Item -LiteralPath $written `
                                    -Destination "$written.$restarts" -Force
                            }
                        }
                    }
                    # `restart:ARG|ARG` starts it again with those arguments
                    # after its own, for a program that is to come back up
                    # differently from how it first started.
                    $again = $launchLine
                    if ($value) {
                        $extra = ($value -split '\|' | ForEach-Object { '"{0}"' -f $_ }) -join ' '
                        $again = "$launchLine $extra"
                    }
                    $proc = Start-Process -FilePath $Exe -ArgumentList $again -PassThru
                    $hwnd = Wait-Until {
                        $seen = [Win32Capture]::FindWindow([uint32]$proc.Id, $Title, $true)
                        if ($seen -ne [IntPtr]::Zero) { $seen }
                    } 30000
                    if (-not $hwnd) { $hwnd = [IntPtr]::Zero }
                    if ($hwnd -eq [IntPtr]::Zero) {
                        throw 'the window did not come back after a restart'
                    }
                    [Win32Capture]::ShowWindow($hwnd, 5) | Out-Null
                    if (-not [Win32Capture]::BringToFront($hwnd)) {
                        Write-Warning 'the restarted window would not come to the front'
                    }
                    $rect = [Win32Capture]::VisibleRect($hwnd)
                    Wait-Loaded $rect
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
                    Start-Sleep -Milliseconds $HAND_MS
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
                    $target = Wait-Until {
                        $seen = [Win32Capture]::FindWindow([uint32]$proc.Id, $wanted, $true)
                        if ($seen -ne [IntPtr]::Zero) { $seen }
                    } 4000
                    if (-not $target) { $target = [IntPtr]::Zero }
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
                    # A dialog that has only just opened has drawn nothing
                    # yet, and has no field for the keys that follow to go to.
                    if ($target -ne $hwnd) {
                        Wait-Drawn $rect
                        Start-Sleep -Milliseconds $HAND_MS
                    }
                    Write-Host ("window: $wanted at {0},{1} {2}x{3}" -f
                        $rect.Left, $rect.Top,
                        ($rect.Right - $rect.Left), ($rect.Bottom - $rect.Top))
                }
                'dragto' {
                    # `dragto:W,H,MS` drags the window's bottom right corner
                    # until the drawable area is exactly W by H, whatever size
                    # the window started at, so the expectations afterwards
                    # can name exact numbers without a size being set by
                    # anything but the mouse.
                    $parts = $value -split ','
                    if ($parts.Count -ne 3) { throw "cannot read dragto: $value" }
                    $client = [Win32Capture]::ClientRect($hwnd)
                    $dx = [int]$parts[0] - ($client.Right - $client.Left)
                    $dy = [int]$parts[1] - ($client.Bottom - $client.Top)
                    $area = [Win32Capture]::OuterRect($hwnd)
                    [Win32Capture]::DragCorner(
                        $area.Right - 3, $area.Bottom - 3, $dx, $dy, [int]$parts[2])
                    # The window is still working through the last of the
                    # pointer's moves when the button comes up, so it is looked
                    # at until it is the size it was dragged to. One that never
                    # gets there is left for the expectations to refuse.
                    Wait-Until {
                        $now = [Win32Capture]::ClientRect($hwnd)
                        (($now.Right - $now.Left) -eq [int]$parts[0]) -and
                            (($now.Bottom - $now.Top) -eq [int]$parts[1])
                    } 2000 | Out-Null
                    $rect = [Win32Capture]::VisibleRect($hwnd)
                    $hostRect = $rect
                    Write-Host ("dragto: {0}x{1} over {2}ms" -f $parts[0], $parts[1], $parts[2])
                }
                'geometry' {
                    # `geometry:PATH` writes down what the window under test and
                    # the plugin's window inside it actually measure, so a test
                    # can assert on it rather than on a picture of it.
                    #
                    # Measured once the plugin's window reaches the host's right
                    # and bottom edges, which after a drag it is expected to,
                    # or as it stands when it never does.
                    Wait-Until {
                        $c = [Win32Capture]::ClientRect($hwnd)
                        $e = [Win32Capture]::EditorInHost($hwnd)
                        ($e.Right -eq ($c.Right - $c.Left)) -and ($e.Bottom -eq ($c.Bottom - $c.Top))
                    } 2000 | Out-Null
                    $client = [Win32Capture]::ClientRect($hwnd)
                    $editor = [Win32Capture]::EditorInHost($hwnd)
                    $w = $client.Right - $client.Left
                    $h = $client.Bottom - $client.Top
                    # How much of the window the plugin does not cover, on each
                    # side. A test after a drag cannot know what size the window
                    # ended up, but it can say the plugin still fills it.
                    $text = (
                        "host={0}x{1}`neditor={2},{3},{4}x{5}`ninset={2},{3},{6},{7}`n" -f
                        $w, $h,
                        $editor.Left, $editor.Top,
                        ($editor.Right - $editor.Left), ($editor.Bottom - $editor.Top),
                        ($w - $editor.Right), ($h - $editor.Bottom))
                    [System.IO.File]::WriteAllText($value, $text)
                    Write-Host "geometry: $($text -replace "`n", ' ')"
                }
                'remove' {
                    if (Test-Path -LiteralPath $value) {
                        Remove-Item -LiteralPath $value -Force
                    }
                }
                'written' {
                    # `written:PATH|TEXT` checks a file in code, mid-test,
                    # right when the step before it claims to have written it.
                    $p, $want = $value -split '\|', 2
                    $want = $want.Trim().Replace('\n', "`n")
                    # The save runs off the plugin's drawing thread, so the file
                    # is looked for until it is there and holds what it should.
                    $there = Wait-Until {
                        (Test-Path -LiteralPath $p) -and
                            [System.IO.File]::ReadAllText($p).Contains($want)
                    }
                    if (-not (Test-Path -LiteralPath $p)) {
                        throw "$p was not written"
                    }
                    $got = [System.IO.File]::ReadAllText($p)
                    if (-not $there) {
                        throw "$p holds `"$got`", expected it to contain `"$want`""
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
                    # `film:PATH|W,H` records a region of that size instead of
                    # the window's own. A window that is about to grow needs it:
                    # the region is fixed for the whole recording, so anything
                    # the window grows into is otherwise never filmed.
                    $filmTo, $filmSize = $value -split '\|', 2
                    $filmVideo = "$filmTo.mkv"
                    Remove-Item -LiteralPath $filmVideo -Force -ErrorAction SilentlyContinue
                    # Always the window under test, whatever the steps are
                    # addressing: anything it opens is on top of it, so one
                    # region catches the lot. Addressing a window brings it to
                    # the front, and doing that to the window behind a modal
                    # would stage the very fight this is here to catch.
                    $area = [Win32Capture]::VisibleRect($hwnd)
                    $fw = $area.Right - $area.Left
                    $fh = $area.Bottom - $area.Top
                    if ($filmSize) {
                        $wh = $filmSize -split ','
                        if ($wh.Count -ne 2) { throw "cannot read film size: $filmSize" }
                        $fw = [int]$wh[0]
                        $fh = [int]$wh[1]
                    }
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
                    # Recording has begun once there is something in the file.
                    $rolling = Wait-Until {
                        (Test-Path -LiteralPath $filmVideo) -and
                            ((Get-Item -LiteralPath $filmVideo).Length -gt 0)
                    }
                    if (-not $rolling) { throw "ffmpeg never started writing $filmVideo" }
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

    # The picture is always of the window under test, whichever window the
    # steps were last addressing. A killed program has no window left.
    if (-not $proc.HasExited) {
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
