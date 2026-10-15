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
    # A sequence of actions, run in order before the grab. No step names a
    # position: a place is named by what is drawn there, and found by
    # photographing the window and reading the picture.
    #
    # TEXT names words the window shows, with qualifiers after a `|`:
    #   TEXT|smallest, TEXT|largest  the one place with that lettering, for
    #                 words the window shows at two sizes, a tab and the
    #                 heading under it
    #   TEXT|below LABEL  only the places under the topmost LABEL
    # WHERE names a point:
    #   TEXT          the middle of TEXT
    #   before TEXT   just left of the line TEXT is on
    #   after TEXT    the empty space right of the line TEXT is on, before the
    #                 next thing drawn on that row or the window's edge
    #   below TEXT    the empty space under the line TEXT is on, before the
    #                 next thing drawn under it or the window's bottom
    #   beside TEXT left, beside TEXT right  the thing drawn beside TEXT on
    #                 its row, past the rest of the label TEXT is part of
    #                 and past its frame: a glyph on a button, a task's box,
    #                 a swatch, a dialog's close cross
    #   between TEXT and TEXT  midway between the two
    # with TEXT's qualifiers after it.
    #
    #   click:WHERE   left click there
    #   type:TEXT     send TEXT as key presses, one at a time
    #   remove:PATH   delete a file, so a save does not hit "already exists"
    #   picture:PATH|W,H  write a PNG of that size to drop or paste
    #   clipboard:PATH    put that picture on the clipboard
    #   copied:TEXT   fail unless the text on the clipboard holds TEXT, `\n`
    #                 for a newline; what a select-all and copy put there
    #   geometry:PATH write down what the window and the plugin inside it measure
    #   hold:WHERE    press the button there and keep holding it
    #   moveto:WHERE  move the pointer there, at the speed a hand moves
    #   letgo:        release the button where the pointer is
    #   letgo:WHERE   move there and release the button
    #   press:TEXT    click the control that says TEXT
    #   showing:TEXT  fail unless the window shows TEXT
    #   hidden:TEXT   fail if the window shows TEXT
    #   dialog:WORD   fail unless a file dialog with WORD on it is open
    #   nodialog:WORD fail if a file dialog is open at all
    #   nowindow:TITLE fail while a window with that title is still there
    #   cursor:SHAPE  fail unless the cursor where the pointer is has that
    #                 shape: arrow, ibeam, hand or grabbing
    #   cursor:WHERE|SHAPE  park the pointer there first
    #   mark:before A, mark:after A  fail unless the drop mark, an upright
    #                 line in the accent colour, is drawn left or right of
    #                 the words A on their row
    #   dragtext:TEXT|to WHERE  drag from TEXT's first letter to WHERE
    #   dbltext:TEXT  double-click the middle of TEXT
    #   righttext:TEXT  right-click the middle of TEXT
    #   dragwords:TEXT|to WHERE  double-click TEXT and drag the second press
    #                 along its row to WHERE
    #   write:PATH|TEXT   write TEXT to a file, `\n` for a newline
    #   dragfile:PATH|WHERE  open PATH's folder in Explorer, pick the file up
    #                 with the mouse, carry it across the screen and drop it
    #                 at WHERE inside the window
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
    /// Two clicks where a finger makes them, close enough together for the
    /// window to take them as one double click.
    public static void DoubleClick(int x, int y) {
        Click(x, y);
        System.Threading.Thread.Sleep(HELD_MS);
        mouse_event(0x0002, 0, 0, 0, System.IntPtr.Zero); // left down
        System.Threading.Thread.Sleep(HELD_MS);
        mouse_event(0x0004, 0, 0, 0, System.IntPtr.Zero); // left up
    }

    /// A click and then a press that stays held, until `LetGo`: the start of
    /// a drag that selects by words.
    public static void DoubleHold(int x, int y) {
        Click(x, y);
        System.Threading.Thread.Sleep(HELD_MS);
        mouse_event(0x0002, 0, 0, 0, System.IntPtr.Zero); // left down
    }

    /// A click with the right button.
    public static void RightClick(int x, int y) {
        SetCursorPos(x, y);
        System.Threading.Thread.Sleep(HELD_MS);
        mouse_event(0x0008, 0, 0, 0, System.IntPtr.Zero); // right down
        System.Threading.Thread.Sleep(HELD_MS);
        mouse_event(0x0010, 0, 0, 0, System.IntPtr.Zero); // right up
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

    /// Carry what is under one point to another, the way a file is dragged
    /// out of one window and dropped on another: press on it, cross the
    /// screen at the speed a hand moves, rest over the target long enough
    /// for the window there to say whether it will take it, and let go.
    public static void Carry(int fromX, int fromY, int toX, int toY) {
        TakeHold(fromX, fromY);
        System.Threading.Thread.Sleep(HELD_MS);
        Glide(fromX, fromY, toX, toY, 600);
        System.Threading.Thread.Sleep(300);
        mouse_event(0x0004, 0, 0, 0, System.IntPtr.Zero); // left up
    }

    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int index);

    /// The screen's size in pixels.
    public static POINT ScreenSize() {
        POINT size;
        size.X = GetSystemMetrics(0);   // SM_CXSCREEN
        size.Y = GetSystemMetrics(1);   // SM_CYSCREEN
        return size;
    }

    /// Put a window at a place on the screen, at a size, without raising it.
    public static void Place(IntPtr hWnd, int x, int y, int w, int h) {
        SetWindowPos(hWnd, IntPtr.Zero, x, y, w, h, 0x0004 | 0x0010);  // SWP_NOZORDER | SWP_NOACTIVATE
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

    /// The visible top-level window of this class with exactly this title,
    /// whichever process owns it: an Explorer window is the shell's, not the
    /// program's that asked for it.
    public static IntPtr FindWindowOf(string className, string title) {
        IntPtr match = IntPtr.Zero;
        EnumWindows((h, l) => {
            if (!IsWindowVisible(h)) return true;
            var kind = new StringBuilder(64);
            GetClassNameW(h, kind, 64);
            if (kind.ToString() != className) return true;
            var text = new StringBuilder(256);
            GetWindowTextW(h, text, 256);
            if (text.ToString() != title) return true;
            match = h;
            return false;
        }, IntPtr.Zero);
        return match;
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
#
# With `$sized` given, `smallest` or `largest`, the finder is asked for the one
# place with that lettering and that is all that comes back.
function Find-All([string]$path, [string]$label, [string]$sized = '') {
    $finder = Join-Path $PSScriptRoot '..\binaries\find-text.exe'
    if (-not (Test-Path $finder)) {
        throw "the text finder is not built: run tools\find-text\build.bat"
    }
    $answer = if ($sized) { @(& $finder $path $label $sized) } else { @(& $finder $path $label) }
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
#
# With `$sized` given, the match is the one with that lettering, and where it
# is does not come into it.
function Find-Label([string]$path, [string]$label, $near, [string]$sized = '') {
    $found = @(Find-All $path $label $sized)
    if (-not $found.Count) { return $null }
    if ($sized -or -not $near) { return $found[0] }
    return $found | Sort-Object {
        [Math]::Pow($_.X - $near.X, 2) + [Math]::Pow($_.Y - $near.Y, 2)
    } | Select-Object -First 1
}

# A step's words, and the qualifiers written after them, each after a `|`.
function Split-Named([string]$value) {
    $pieces = @($value -split '\|' | ForEach-Object { $_.Trim() })
    return @{ Named = $pieces[0]; Qualifiers = @($pieces | Select-Object -Skip 1) }
}

# Every place TEXT is read in a picture, best match first, kept to the
# qualifiers: `smallest` or `largest` is the one place with that lettering,
# and `below LABEL` keeps the places under the topmost LABEL.
function Find-Qualified([string]$path, [string]$text, [string[]]$qualifiers) {
    $sized = ''
    foreach ($q in $qualifiers) {
        if ($q -in 'smallest', 'largest') { $sized = $q }
    }
    $found = @(Find-All $path $text $sized)
    foreach ($q in $qualifiers) {
        if ($q -like 'below *') {
            $anchor = @(Find-All $path $q.Substring(6).Trim() | Sort-Object { $_.Top })[0]
            if (-not $anchor) { return @() }
            $found = @($found | Where-Object { $_.Top -ge $anchor.Bottom })
        }
    }
    return $found
}

# The first line read in a picture that holds TEXT, boxed across the whole
# line, kept to the qualifiers the way Find-Qualified keeps places.
function Find-LineOf([string]$path, [string]$text, [string[]]$qualifiers) {
    $wanted = $text.ToLower()
    $lines = @(Read-Text $path | Where-Object { $_.Text.ToLower().Contains($wanted) })
    foreach ($q in $qualifiers) {
        if ($q -like 'below *') {
            $anchor = @(Find-All $path $q.Substring(6).Trim() | Sort-Object { $_.Top })[0]
            if (-not $anchor) { return $null }
            $lines = @($lines | Where-Object { $_.Top -ge $anchor.Bottom })
        }
        if ($q -eq 'smallest') { $lines = @($lines | Sort-Object { $_.Bottom - $_.Top }) }
        if ($q -eq 'largest') { $lines = @($lines | Sort-Object { $_.Top - $_.Bottom }) }
    }
    return $lines[0]
}

# Something is drawn in a column where the column's pixels vary down the band
# of rows the words are on: letters, a glyph, the outline of a box, the edge
# of a swatch all vary. A fill, a frame line, a separator and a caret run
# straight down the band in one colour and vary nowhere, so they are not
# things, whatever colour they or the ground around them are.
#
# How far two colours have to be apart to be two colours; how many rows above
# and below the words' own rows the band takes in, so the top and bottom of a
# box drawn round the words' height are in it and the frame of the tab or
# button the words are on is not; how many empty columns or rows in a row end
# the words, the gaps inside a word being narrower; how many empty columns may
# sit inside one thing drawn; and how wide a thing has to be to be the thing
# rather than a line or the smoothing of one. A name here is one name in any
# case, so none of these is a word a counter below is called by.
$COLOURS_APART = 60
$BAND_ROWS = 3
$GAP_ENDS_WORDS = 4
$GAP_WITHIN_THING = 3
$THING_WIDTH = 5

function Test-Apart($a, $b) {
    return ([Math]::Abs($a.R - $b.R) + [Math]::Abs($a.G - $b.G) + [Math]::Abs($a.B - $b.B)) -gt $COLOURS_APART
}

# Whether column $x is drawn in: its pixels vary between rows $y0 and $y1.
function Test-ColumnVaries($bmp, [int]$x, [int]$y0, [int]$y1) {
    if ($x -lt 0 -or $x -ge $bmp.Width) { return $false }
    $y0 = [Math]::Max(0, $y0)
    $y1 = [Math]::Min($bmp.Height, $y1)
    if ($y1 -le $y0) { return $false }
    $first = $bmp.GetPixel($x, $y0)
    for ($y = $y0 + 1; $y -lt $y1; $y++) {
        if (Test-Apart $bmp.GetPixel($x, $y) $first) { return $true }
    }
    return $false
}

# Whether row $y is drawn in: its pixels vary between columns $x0 and $x1.
function Test-RowVaries($bmp, [int]$y, [int]$x0, [int]$x1) {
    if ($y -lt 0 -or $y -ge $bmp.Height) { return $false }
    $x0 = [Math]::Max(0, $x0)
    $x1 = [Math]::Min($bmp.Width, $x1)
    if ($x1 -le $x0) { return $false }
    $first = $bmp.GetPixel($x0, $y)
    for ($x = $x0 + 1; $x -lt $x1; $x++) {
        if (Test-Apart $bmp.GetPixel($x, $y) $first) { return $true }
    }
    return $false
}

# The rows the words really take up: from the middle of their box up and
# down, every row drawn in across the box's columns. The finder's box can be
# taller than the letters, and a band taken from it would reach the frame of
# the tab or button the words are on.
function Get-RowsOf($bmp, $box) {
    $top = [int]$box.Y
    while ($top -gt 0 -and (Test-RowVaries $bmp ($top - 1) $box.Left $box.Right)) { $top-- }
    $bottom = [int]$box.Y
    while ($bottom + 1 -lt $bmp.Height -and (Test-RowVaries $bmp ($bottom + 1) $box.Left $box.Right)) { $bottom++ }
    return @{ Top = $top; Bottom = $bottom + 1 }
}

# The columns from the words' edge outward, past the words themselves: the
# first $GAP_ENDS_WORDS empty columns in a row end them. The column the words end in,
# and every column beyond the gap with whether it is drawn in.
function Get-ColumnsBeyond($bmp, $box, [string]$side) {
    $rows = Get-RowsOf $bmp $box
    $y0 = $rows.Top - $BAND_ROWS
    $y1 = $rows.Bottom + $BAND_ROWS
    $step = if ($side -eq 'right') { 1 } else { -1 }
    $x = if ($side -eq 'right') { $box.Right - 1 } else { $box.Left }
    # A box wider than the letters starts outside them: step in to the last
    # column drawn in, which is where they end.
    while (($x - $step) -ge $box.Left -and ($x - $step) -lt $box.Right -and
        -not (Test-ColumnVaries $bmp $x $y0 $y1)) {
        $x -= $step
    }
    $empty = 0
    $end = $x
    $columns = New-Object System.Collections.ArrayList
    while ($x -ge 0 -and $x -lt $bmp.Width) {
        $drawn = Test-ColumnVaries $bmp $x $y0 $y1
        if ($empty -lt $GAP_ENDS_WORDS) {
            if ($drawn) { $empty = 0; $end = $x } else { $empty++ }
        } else {
            [void]$columns.Add(@{ X = $x; Drawn = $drawn })
        }
        $x += $step
    }
    return @{ End = $end; Columns = $columns }
}

# How many empty columns in a row end a label: more than the space between
# two words of one label, fewer than the gap between one control and the next.
$GAP_ENDS_LABEL = 10

# The words widened to the rest of the label they are part of: the columns
# drawn on the line they were read on, toward $side, until a gap wider than a
# word's space. A button's label is read as several words, and the thing
# beside the button is past all of them. The line's own edge bounds it.
function Get-LabelOf($bmp, $box, $line, [string]$side) {
    $label = @{}
    foreach ($key in $box.Keys) { $label[$key] = $box[$key] }
    if (-not $line) { return $label }
    $rows = Get-RowsOf $bmp $box
    $y0 = $rows.Top - $BAND_ROWS
    $y1 = $rows.Bottom + $BAND_ROWS
    $step = if ($side -eq 'right') { 1 } else { -1 }
    $x = if ($side -eq 'right') { $box.Right } else { $box.Left - 1 }
    $end = $null
    $empty = 0
    while ($x -ge $line.Left -and $x -lt $line.Right) {
        if (Test-ColumnVaries $bmp $x $y0 $y1) {
            $end = $x
            $empty = 0
        } else {
            $empty++
            if ($empty -ge $GAP_ENDS_LABEL) { break }
        }
        $x += $step
    }
    if ($null -ne $end) {
        if ($side -eq 'right') { $label.Right = $end + 1 } else { $label.Left = $end }
        $label.X = [int](($label.Left + $label.Right) / 2)
    }
    return $label
}

# The line a box of words was read on: the one whose box holds the words'
# middle. Nothing when no line does.
function Find-LineHolding([string]$path, $words) {
    return @(Read-Text $path | Where-Object {
        $_.Left -le $words.X -and $words.X -le $_.Right -and
        $_.Top -le $words.Y -and $words.Y -le $_.Bottom
    })[0]
}

# The middle column of the first thing drawn beside the words on their row,
# past the words themselves and past any line, such as the frame of the
# button the words are on. Nothing when nothing is drawn there.
function Get-Beside($bmp, $box, [string]$side) {
    $run = $null
    foreach ($column in (Get-ColumnsBeyond $bmp $box $side).Columns) {
        if ($column.Drawn) {
            if ($run) { $run.End = $column.X; $run.Gap = 0 }
            else { $run = @{ Start = $column.X; End = $column.X; Gap = 0 } }
        } elseif ($run) {
            $run.Gap++
            if ($run.Gap -gt $GAP_WITHIN_THING) {
                if ([Math]::Abs($run.End - $run.Start) + 1 -ge $THING_WIDTH) {
                    return [int](($run.Start + $run.End) / 2)
                }
                $run = $null
            }
        }
    }
    if ($run -and ([Math]::Abs($run.End - $run.Start) + 1 -ge $THING_WIDTH)) {
        return [int](($run.Start + $run.End) / 2)
    }
    return $null
}

# The middle of the empty space right of a line on its row: between the
# line's end and the next thing drawn there, or the picture's edge.
function Get-After($bmp, $box) {
    $beyond = Get-ColumnsBeyond $bmp $box 'right'
    $next = $bmp.Width
    foreach ($column in $beyond.Columns) {
        if ($column.Drawn) { $next = $column.X; break }
    }
    return [int](($beyond.End + 1 + $next) / 2)
}

# The middle of the empty space under a line, in its columns: between the
# line's bottom and the next thing drawn under it, or the picture's bottom.
# The line's bottom is found from its middle row down, past every row drawn
# in, since the finder's box can be off by a little.
function Get-Below($bmp, $box) {
    $empty = 0
    $end = [int]$box.Y
    $next = $bmp.Height
    for ($y = [int]$box.Y; $y -lt $bmp.Height; $y++) {
        $drawn = Test-RowVaries $bmp $y $box.Left $box.Right
        if ($empty -lt $GAP_ENDS_WORDS) {
            if ($drawn) { $empty = 0; $end = $y } else { $empty++ }
            continue
        }
        if ($drawn) { $next = $y; break }
    }
    return [int](($end + 1 + $next) / 2)
}

# Where a WHERE phrase points in a picture, as X and Y in the picture, or
# nothing when what it names is not in the picture.
function Find-PointIn([string]$path, [string]$phrase, [string[]]$qualifiers) {
    if ($phrase -match '^between (.+) and (.+)$') {
        $one, $other = $Matches[1].Trim(), $Matches[2].Trim()
        $a = @(Find-Qualified $path $one $qualifiers)[0]
        $b = @(Find-Qualified $path $other $qualifiers)[0]
        if (-not $a -or -not $b) { return $null }
        return @{ X = [int](($a.Right + $b.Left) / 2); Y = [int](($a.Y + $b.Y) / 2) }
    }
    if ($phrase -match '^beside (.+) (left|right)$') {
        $text, $side = $Matches[1].Trim(), $Matches[2]
        $words = @(Find-Qualified $path $text $qualifiers)[0]
        if (-not $words) { return $null }
        $line = Find-LineHolding $path $words
        Add-Type -AssemblyName System.Drawing
        $bmp = New-Object System.Drawing.Bitmap $path
        try {
            $label = Get-LabelOf $bmp $words $line $side
            $x = Get-Beside $bmp $label $side
        } finally { $bmp.Dispose() }
        if ($null -eq $x) { return $null }
        return @{ X = $x; Y = $words.Y }
    }
    if ($phrase -match '^(before|after|below) (.+)$') {
        $how, $text = $Matches[1], $Matches[2].Trim()
        $line = Find-LineOf $path $text $qualifiers
        if (-not $line) { return $null }
        if ($how -eq 'before') { return @{ X = $line.Left - 2; Y = $line.Y } }
        Add-Type -AssemblyName System.Drawing
        $bmp = New-Object System.Drawing.Bitmap $path
        try {
            if ($how -eq 'after') { return @{ X = (Get-After $bmp $line); Y = $line.Y } }
            return @{ X = $line.X; Y = (Get-Below $bmp $line) }
        } finally { $bmp.Dispose() }
    }
    $words = @(Find-Qualified $path $phrase $qualifiers)[0]
    if (-not $words) { return $null }
    return @{ X = $words.X; Y = $words.Y }
}

# Read the controls the steps name beside words while the window is clean
# and still, as it is when it has just come up: each `beside` phrase in the
# steps is found on one picture and said, before any step runs.
function Confirm-Buttons([Win32Capture+RECT]$area, [string[]]$steps) {
    $phrases = @{}
    foreach ($step in $steps) {
        foreach ($match in [regex]::Matches($step, 'beside [^|]+ (left|right)(\|[^|]+)*')) {
            $value = $match.Value
            foreach ($cut in '\|hand$', '\|ibeam$', '\|arrow$', '\|grabbing$') {
                $value = $value -replace $cut, ''
            }
            $phrases[$value] = $true
        }
    }
    if ($phrases.Count -eq 0) { return }
    $probe = Join-Path $env:TEMP 'capture-window-buttons.png'
    Save-Shot $area $probe
    foreach ($value in $phrases.Keys) {
        $parts = Split-Named $value
        $found = Find-PointIn $probe $parts.Named $parts.Qualifiers
        if ($found) {
            Write-Host ("button: {0} at {1},{2}" -f $value, $found.X, $found.Y)
        } else {
            Write-Host ("button: {0} is not on the first picture" -f $value)
        }
    }
    Remove-Item -LiteralPath $probe -Force -ErrorAction SilentlyContinue
}

# How far a pixel's strongest channel is above its weakest for the pixel to
# be the drop mark's colour: the accent, a medium blue, where the frames and
# the strip are greys and a tab's fill and the ghost of one are pale.
$MARK_COLOURED = 70

# Whether column $x is a line of the mark's colour down the band: every pixel
# from $y0 to $y1 that strongly coloured.
function Test-ColumnMarked($bmp, [int]$x, [int]$y0, [int]$y1) {
    if ($x -lt 0 -or $x -ge $bmp.Width) { return $false }
    for ($y = [Math]::Max(0, $y0); $y -lt [Math]::Min($bmp.Height, $y1); $y++) {
        $p = $bmp.GetPixel($x, $y)
        $spread = [Math]::Max($p.R, [Math]::Max($p.G, $p.B)) - [Math]::Min($p.R, [Math]::Min($p.G, $p.B))
        if ($spread -le $MARK_COLOURED) { return $false }
    }
    return $true
}

# Where the drop mark is in a picture, within a span of columns, on the rows
# of the words the span is measured from. Nothing when there is none.
function Find-MarkIn([string]$path, $words, [int]$x0, [int]$x1) {
    Add-Type -AssemblyName System.Drawing
    $bmp = New-Object System.Drawing.Bitmap $path
    try {
        $rows = Get-RowsOf $bmp $words
        for ($x = [Math]::Max(0, $x0); $x -lt [Math]::Min($bmp.Width, $x1); $x++) {
            if (Test-ColumnMarked $bmp $x $rows.Top $rows.Bottom) {
                return @{ X = $x; Y = [int]$words.Y }
            }
        }
    } finally { $bmp.Dispose() }
    return $null
}

# The point on screen a WHERE phrase names, looked for in a fresh picture
# until it is there: what it names may be what the step before has only just
# caused to be drawn. A failure says what the window reads instead.
function Resolve-Point([Win32Capture+RECT]$area, [string]$value, [string]$step) {
    $parts = Split-Named $value
    $probe = Join-Path $env:TEMP 'capture-window-point.png'
    $found = Wait-Until {
        Save-Shot $area $probe
        Find-PointIn $probe $parts.Named $parts.Qualifiers
    }
    if (-not $found) {
        $saw = (Read-Text $probe | ForEach-Object { $_.Text }) -join ' | '
        throw "nothing is at `"$value`" for ${step}; the window reads: $saw; see $probe"
    }
    # The picture the point was found in stays beside the test's own, so a
    # point that landed wrong can be read against it.
    if ($outDir) {
        Move-Item -LiteralPath $probe -Destination (Join-Path $outDir 'capture-window-point.png') -Force
    }
    return @{ X = $area.Left + [int]$found.X; Y = $area.Top + [int]$found.Y }
}

# The box of the words a step names, in the window, looked for until it is
# there. A failure says what the window reads instead, so a word drawn
# elsewhere can be told from a word the reader could not make out.
function Find-Words([Win32Capture+RECT]$area, [string]$value, [string]$step) {
    $parts = Split-Named $value
    $probe = Join-Path $env:TEMP 'capture-window-words.png'
    $found = Wait-Until {
        Save-Shot $area $probe
        @(Find-Qualified $probe $parts.Named $parts.Qualifiers)[0]
    }
    if (-not $found) {
        $saw = (Read-Text $probe | ForEach-Object { $_.Text }) -join ' | '
        throw "`"$value`" is not on screen for ${step}; the window reads: $saw; see $probe"
    }
    Remove-Item -LiteralPath $probe -Force -ErrorAction SilentlyContinue
    return $found
}

# Where ffmpeg is. Recording a window is its job, and screenshots cannot do it:
# a flicker lasting a frame or two is gone between two grabs half a second
# apart. gdigrab takes every frame the window draws.

# Somewhere on the screen for a window `w` by `h` that overlaps `$taken`
# nowhere: to its right, to its left, under it or above it, the first of
# those with the room. A file carried from one window to another must cross
# no part of the window it came from on the way.
function Find-Room([Win32Capture+RECT]$taken, [int]$w, [int]$h) {
    $screen = [Win32Capture]::ScreenSize()
    $tries = @(
        @{ X = $taken.Right; Y = $taken.Top },
        @{ X = $taken.Left - $w; Y = $taken.Top },
        @{ X = $taken.Left; Y = $taken.Bottom },
        @{ X = $taken.Left; Y = $taken.Top - $h }
    )
    foreach ($try in $tries) {
        $x = [Math]::Max(0, [Math]::Min($try.X, $screen.X - $w))
        $y = [Math]::Max(0, [Math]::Min($try.Y, $screen.Y - $h))
        $clear = ($x -ge $taken.Right) -or ($x + $w -le $taken.Left) -or
            ($y -ge $taken.Bottom) -or ($y + $h -le $taken.Top)
        if ($clear) { return @{ X = $x; Y = $y } }
    }
    throw "no room on the screen for a ${w}x${h} window beside the one under test"
}

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
        Confirm-Buttons $rect $Steps
        foreach ($step in $Steps) {
            $kind, $value = $step -split ':', 2
            switch ($kind) {
                'click' {
                    $at = Resolve-Point $rect $value 'click'
                    [Win32Capture]::Click($at.X, $at.Y)
                    Start-Sleep -Milliseconds $HAND_MS
                    Write-Host ("click:  {0} at {1},{2}" -f $value, ($at.X - $rect.Left), ($at.Y - $rect.Top))
                }
                'press' {
                    # `press:TEXT` looks first and clicks second: the window
                    # is photographed, the words are found in the picture, and
                    # the click goes where they really are. Words the window
                    # does not show are a failure, not a blind click.
                    $found = Find-Words $rect $value 'press'
                    [Win32Capture]::MoveTo(
                        $rect.Left + [int]$found.X, $rect.Top + [int]$found.Y)
                    [Win32Capture]::Click(
                        $rect.Left + [int]$found.X, $rect.Top + [int]$found.Y)
                    Start-Sleep -Milliseconds $HAND_MS
                    Write-Host ("press:  {0} at {1},{2}" -f $value, $found.X, $found.Y)
                }
                'type' {
                    Send-Typing $value
                }
                { $_ -in 'showing', 'hidden' } {
                    # `showing:TEXT` reads the window and fails unless TEXT is
                    # there; `hidden:` fails if it is.
                    $parts = Split-Named $value
                    if (-not $parts.Named) { throw "cannot read ${kind}: $value" }
                    $probe = Join-Path $env:TEMP 'capture-window-showing.png'
                    try {
                        # Looked at until it is as the step says: what the step
                        # before did may not have been drawn yet, and a tooltip
                        # takes its time coming and going.
                        $asSaid = Wait-Until {
                            Save-Shot $rect $probe
                            $there = @(Find-Qualified $probe $parts.Named $parts.Qualifiers).Count -gt 0
                            $there -eq ($kind -eq 'showing')
                        }
                        $found = if ($asSaid) { $kind -eq 'showing' } else { $kind -ne 'showing' }
                        # Only a step that is about to fail reads the window
                        # again, to say what it holds instead.
                        $saw = ''
                        if (-not $asSaid) {
                            $saw = (Read-Text $probe | ForEach-Object { $_.Text }) -join ' | '
                        }
                    } finally {
                        Remove-Item -LiteralPath $probe -Force -ErrorAction SilentlyContinue
                    }
                    if ($kind -eq 'showing' -and -not $found) {
                        throw "the window does not show `"$value`"; it reads: $saw"
                    }
                    if ($kind -eq 'hidden' -and $found) {
                        throw "the window still shows `"$value`"; it reads: $saw"
                    }
                    Write-Host ("{0}: {1}" -f $kind, $value)
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
                { $_ -in 'dragtext', 'dragwords' } {
                    # `dragtext:TEXT|to WHERE` selects by dragging from TEXT's
                    # first letter to WHERE. `dragwords:TEXT|to WHERE`
                    # double-clicks the middle of TEXT and drags the second
                    # press along TEXT's row to WHERE: a drag that selects by
                    # words. The pieces before the `to` qualify TEXT, the
                    # pieces after it qualify WHERE.
                    $pieces = @($value -split '\|' | ForEach-Object { $_.Trim() })
                    $at = [Array]::FindIndex($pieces, [Predicate[string]]{ param($p) $p -like 'to *' })
                    if ($at -lt 1) { throw "cannot read ${kind}: $value" }
                    $named = ($pieces[0..($at - 1)]) -join '|'
                    $target = (@($pieces[$at].Substring(3)) + @($pieces | Select-Object -Skip ($at + 1))) -join '|'
                    $found = Find-Words $rect $named $kind
                    $row = $rect.Top + [int]$found.Y
                    $to = Resolve-Point $rect $target $kind
                    if ($kind -eq 'dragtext') {
                        # On the first glyph, not beside it: a press lands on
                        # the nearest character boundary, and the boundary
                        # before the first letter is the one its ink starts at.
                        [Win32Capture]::Drag($rect.Left + [int]$found.Left, $row, $to.X, $to.Y)
                    } else {
                        [Win32Capture]::DoubleHold($rect.Left + [int]$found.X, $row)
                        [Win32Capture]::LetGo($to.X, $row)
                    }
                    Start-Sleep -Milliseconds $HAND_MS
                    Write-Host ("{0}: {1}" -f $kind, $value)
                }
                'mark' {
                    # `mark:before A` and `mark:after A` fail unless the drop
                    # mark, an upright line in the accent colour, is drawn
                    # left or right of the words A on their row.
                    $parts = Split-Named $value
                    $phrase = $parts.Named
                    $probe = Join-Path $env:TEMP 'capture-window-mark.png'
                    $found = Wait-Until {
                        Save-Shot $rect $probe
                        if ($phrase -match '^before (.+)$') {
                            $a = @(Find-Qualified $probe $Matches[1].Trim() $parts.Qualifiers)[0]
                            if ($a) { Find-MarkIn $probe $a 0 $a.Left }
                        } elseif ($phrase -match '^after (.+)$') {
                            $a = @(Find-Qualified $probe $Matches[1].Trim() $parts.Qualifiers)[0]
                            if ($a) { Find-MarkIn $probe $a $a.Right ($rect.Right - $rect.Left) }
                        } else {
                            throw "cannot read mark: $value"
                        }
                    }
                    if (-not $found) {
                        throw "no drop mark is drawn $value; see $probe"
                    }
                    Remove-Item -LiteralPath $probe -Force -ErrorAction SilentlyContinue
                    Write-Host ("mark:   {0} at {1},{2}" -f $value, $found.X, $found.Y)
                }
                { $_ -in 'dbltext', 'righttext' } {
                    # `dbltext:TEXT` double-clicks the middle of TEXT, so the
                    # press is inside the word and not on its edge;
                    # `righttext:TEXT` clicks it with the right button.
                    $found = Find-Words $rect $value $kind
                    if ($kind -eq 'dbltext') {
                        [Win32Capture]::DoubleClick($rect.Left + [int]$found.X, $rect.Top + [int]$found.Y)
                    } else {
                        [Win32Capture]::RightClick($rect.Left + [int]$found.X, $rect.Top + [int]$found.Y)
                    }
                    Start-Sleep -Milliseconds $HAND_MS
                    Write-Host ("{0}: {1}" -f $kind, $value)
                }
                'hold' {
                    # `hold:WHERE` presses and keeps holding, so the steps
                    # after it happen mid-drag.
                    $at = Resolve-Point $rect $value 'hold'
                    [Win32Capture]::TakeHold($at.X, $at.Y)
                    Write-Host ("hold:   {0}" -f $value)
                }
                'moveto' {
                    # `moveto:WHERE` moves the pointer there, button or no
                    # button.
                    $at = Resolve-Point $rect $value 'moveto'
                    [Win32Capture]::MoveTo($at.X, $at.Y)
                    Write-Host ("moveto: {0}" -f $value)
                }
                'letgo' {
                    # `letgo:` releases the button where the pointer is;
                    # `letgo:WHERE` moves there first.
                    if ($value.Trim() -eq '') {
                        $now = New-Object Win32Capture+POINT
                        [Win32Capture]::GetCursorPos([ref]$now) | Out-Null
                        $at = @{ X = $now.X; Y = $now.Y }
                    } else {
                        $at = Resolve-Point $rect $value 'letgo'
                    }
                    [Win32Capture]::LetGo($at.X, $at.Y)
                    Start-Sleep -Milliseconds $HAND_MS
                    Write-Host ("letgo:  {0}" -f $value)
                }
                'cursor' {
                    # `cursor:SHAPE` checks what the window asked the cursor
                    # to be where the pointer is; `cursor:WHERE|SHAPE` parks
                    # the pointer at WHERE first.
                    $pieces = @($value -split '\|' | ForEach-Object { $_.Trim() })
                    $want = $pieces[-1]
                    $where = ($pieces[0..($pieces.Count - 2)]) -join '|'
                    if ($pieces.Count -ge 2) {
                        $at = Resolve-Point $rect $where 'cursor'
                        [Win32Capture]::SetCursorPos($at.X, $at.Y) | Out-Null
                    } else {
                        $now = New-Object Win32Capture+POINT
                        [Win32Capture]::GetCursorPos([ref]$now) | Out-Null
                        $at = @{ X = $now.X; Y = $now.Y }
                        $where = 'the pointer'
                    }
                    # The window only changes it when it next redraws, so it is
                    # looked at until it is what was asked for.
                    $right = Wait-Until { [Win32Capture]::CursorNow() -eq $want } 2000
                    if (-not $right) {
                        $shown = [Win32Capture]::CursorNow()
                        throw ("cursor at $where, which is {0},{1}, is $shown, expected $want" -f
                            ($at.X - $rect.Left), ($at.Y - $rect.Top))
                    }
                    Write-Host ("cursor: {0} is {1}" -f $where, $want)
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
                    # A letter is its own virtual key; a named key is looked up.
                    $letter = $parts[1]
                    $key = switch ($letter) {
                        'home' { 0x24 }
                        'left' { 0x25 }
                        default {
                            if ($letter.Length -ne 1) { throw "unknown key: $letter" }
                            [uint16][char]($letter.ToUpper())
                        }
                    }
                    [Win32Capture]::Shortcut([uint16]$modifier, [uint16]$key)
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
                    Confirm-Buttons $rect $Steps
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
                'picture' {
                    # `picture:PATH|W,H` writes a PNG of that size, all one
                    # colour, for a test to drop or paste.
                    Add-Type -AssemblyName System.Drawing
                    $p, $size = $value -split '\|', 2
                    $w, $h = $size -split ','
                    $bmp = New-Object System.Drawing.Bitmap ([int]$w), ([int]$h)
                    $gfx = [System.Drawing.Graphics]::FromImage($bmp)
                    $gfx.Clear([System.Drawing.Color]::FromArgb(255, 200, 40, 160))
                    $gfx.Dispose()
                    $bmp.Save($p, [System.Drawing.Imaging.ImageFormat]::Png)
                    $bmp.Dispose()
                }
                'clipboard' {
                    # `clipboard:PATH` puts that picture on the clipboard, as
                    # a screenshot tool does, for a paste to find.
                    Add-Type -AssemblyName System.Drawing
                    $held = [System.Drawing.Image]::FromFile($value)
                    [System.Windows.Forms.Clipboard]::SetImage($held)
                    $held.Dispose()
                }
                'copied' {
                    # `copied:TEXT` reads the text on the clipboard and fails
                    # unless TEXT is in it. The program writes the clipboard
                    # a frame after the key, so it is read until it holds
                    # TEXT or the wait runs out. Line ends are read as `\n`
                    # whatever the clipboard wrote them as.
                    $wanted = "$value".Replace('\n', "`n")
                    if (-not $wanted) { throw "cannot read copied: $value" }
                    $held = ''
                    $found = Wait-Until {
                        $held = "$([System.Windows.Forms.Clipboard]::GetText())".Replace("`r`n", "`n")
                        $held.Contains($wanted)
                    }
                    if (-not $found) {
                        throw "the clipboard does not hold `"$value`"; it holds: $held"
                    }
                    Write-Host ("copied: {0}" -f $value)
                }
                'write' {
                    # `write:PATH|TEXT` writes TEXT to a file, `\n` for a
                    # newline, for a test to drag or open.
                    $p, $text = $value -split '\|', 2
                    $p = $p.Trim()
                    if (-not $p) { throw "cannot read write: $value" }
                    $dir = Split-Path -Parent $p
                    if ($dir -and -not (Test-Path -LiteralPath $dir)) {
                        New-Item -ItemType Directory -Force $dir | Out-Null
                    }
                    [System.IO.File]::WriteAllText($p, "$text".Replace('\n', "`n"))
                }
                'dragfile' {
                    # `dragfile:PATH|WHERE` drags a file out of Explorer and
                    # drops it at WHERE in the window: the file's folder is
                    # opened in Explorer, the file's name is found in that
                    # window by reading it, the mouse presses on the name,
                    # carries it across the screen to WHERE and lets go there.
                    # Explorer is the source of the drag, as it is when a
                    # person does this, and nothing here is told what is
                    # being dragged. The Explorer window is closed afterwards.
                    $p, $where = $value -split '\|', 2
                    $p = "$p".Trim()
                    if (-not $p -or -not "$where".Trim()) { throw "cannot read dragfile: $value" }
                    if (-not (Test-Path -LiteralPath $p)) { throw "nothing to drag at $p" }
                    # The drop's place is found before Explorer comes up over
                    # anything.
                    $target = Resolve-Point $rect $where.Trim() 'dragfile'
                    $folder = Split-Path -Parent $p
                    $folderName = Split-Path -Leaf $folder
                    # Explorer hides the extension of a file type it knows,
                    # so the name is looked for without it.
                    $stem = [System.IO.Path]::GetFileNameWithoutExtension($p)
                    Start-Process -FilePath explorer.exe -ArgumentList "/select,`"$p`""
                    $source = Wait-Until {
                        $seen = [Win32Capture]::FindWindowOf('CabinetWClass', $folderName)
                        if ($seen -ne [IntPtr]::Zero) { $seen }
                    } 15000
                    if (-not $source) { throw "Explorer never opened $folder" }
                    # Beside the window under test, so the file's trip to
                    # X,Y crosses no part of Explorer: a file let go over
                    # Explorer is dropped there instead.
                    $room = Find-Room $rect 640 480
                    [Win32Capture]::Place($source, $room.X, $room.Y, 640, 480)
                    if (-not [Win32Capture]::BringToFront($source)) {
                        Write-Warning 'Explorer would not come to the front'
                    }
                    $area = [Win32Capture]::ClientRect($source)
                    # The look is kept away from the folder Explorer is
                    # showing until the drop is done: a file appearing in that
                    # folder moves every row under it, and the press lands on
                    # the wrong file.
                    $probe = Join-Path $env:TEMP 'capture-window-dragfile.png'
                    # The leftmost place the name is read: the file list is
                    # left of the details pane, which names the file too, and
                    # a press there picks nothing up.
                    $found = Wait-Until {
                        Save-Shot $area $probe
                        @(Find-All $probe $stem | Sort-Object { $_.X })[0]
                    } 10000
                    if (-not $found) {
                        $saw = (Read-Text $probe | ForEach-Object { $_.Text }) -join ' | '
                        throw "Explorer shows no file called `"$stem`"; it reads: $saw; see $probe"
                    }
                    Write-Host ("dragfile: `"{0}`" read at {1},{2} in Explorer" -f
                        $stem, $found.X, $found.Y)
                    [Win32Capture]::Carry(
                        $area.Left + [int]$found.X, $area.Top + [int]$found.Y,
                        $target.X, $target.Y)
                    Start-Sleep -Milliseconds $HAND_MS
                    # The picture stays beside the test's own, named after the
                    # file, so a press that picked up the wrong row can be read
                    # against it.
                    if ($outDir) {
                        Move-Item -LiteralPath $probe -Destination (Join-Path $outDir "dragfile-$stem.png") -Force
                    }
                    [Win32Capture]::PostMessage($source, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
                    Wait-Until {
                        [Win32Capture]::FindWindowOf('CabinetWClass', $folderName) -eq [IntPtr]::Zero
                    } 5000 | Out-Null
                    if (-not [Win32Capture]::BringToFront($currentHwnd)) {
                        Write-Warning 'the window would not come back to the front'
                    }
                    Write-Host ("dragfile: {0} to {1}" -f $p, $where.Trim())
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
