# Screen layout and controls

Where the Studio views sit and how big they are (`:DOCK`), the actions of the plot's right-click menu (`:VIEW`, `:ZOOM`), and the controls on the screen by name and place (`:UI`). With these a program can work the scope the way a person does: put the measurement table beside the waveform, drag a divider, float the spectrum, open the right-click menu and pick a row, or press any button it read off the screen.

Every command runs the same code as the finger: `:DOCK:MOVE` makes the drop the drag preview offers, `:UI:TAP` and `:UI:DRAG` are touches. All are OOS extensions. The `:DOCK` commands need the Studio layout (`:DISPlay:LAYout STUDio`); in another layout they fail with -221.

Menu location: the title strip of each view (drag, X, expand), the dividers between views, and the right-click (long press) menu of the plot.

Part of the [SCPI reference](README.md).

## Views

A view is named by one of these keywords. Replies use the short form in the first column.

| Name | Long form | The view |
| --- | --- | --- |
| `WAVE` | `WAVEform` | The waveform (or the analyser that replaces it, such as Spectrum) |
| `MEAS` | `MEASurements` | The measurement table |
| `EYE` | `EYE` | The eye diagram |
| `PANE` | `PANE` | The second pane: spectrum, trend, results, events or bus |
| `TABL` | `TABLe` | The decode or search-event table |
| `SIM` | `SIM` | The signal simulator |
| `HIST` | `HISTogram` | The waveform histogram |
| `XY` | `XY` | The XY picture |
| `VIEW` | `VIEW` | An analyser view shown beside the waveform |

Rectangles are `x,y,w,h` in screen pixels from the top-left corner. `:UI:SCReen?` gives the size of the screen.

## Commands

### Docking

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:DOCK:LIST` | Query | Answers one record per view, separated by `;`: `<name>,<state>,x,y,w,h`. The state is `DOCK` (on the screen in the dock), `TAB` (docked behind another tab of its group; the rectangle is the group's), `FLOAT` (a floating panel) or `OFF`. | - | - | OOS extension |
| `:DOCK:MOVE` | Set | `<view>,<target>,{LEFT\|RIGHt\|TOP\|BOTTom\|CENTer\|TABS}`: docks `<view>` against the docked view `<target>`, splitting the target's place 50/50 at that side. `CENTer` swaps the two, `TABS` joins the target's tab group. `<view>,EDGE,<side>` docks it along a whole edge of the area, a third of the room. | - | Drag a view by its title strip | OOS extension |
| `:DOCK:SIZE` | Set / Query | `<view>,<fraction>`, 0.02 to 0.98: moves the divider beside the view so that it has that share of its split. The least sizes of the views still apply (the waveform stays at least 300 x 210). `:DOCK:SIZE? <view>` answers the share, `1` when the view fills the area and `0` when it is floating or off. | - | Drag a divider | OOS extension |
| `:DOCK:FLOAT` | Set / Query | `<view>[,x,y,w,h]`: floats the view at that rectangle (kept on the screen, at least its least size), or where it floated last. `:DOCK:FLOAT? <view>` answers `x,y,w,h`, `0` when it is docked. The last docked view cannot float. | - | Drag a view off the dock; drag a floating panel's corner | OOS extension |
| `:DOCK:SHOW` | Set / Query | `<view>`: shows `MEAS` or `SIM`. The other views open with their own commands (`:EYE...`, `:VIEW`, the decoder and histogram commands); for them the set only checks that they are on. `:DOCK:SHOW? <view>` answers `1` or `0`. | - | View menu | OOS extension |
| `:DOCK:CLOSe` | Set | `<view>`: closes the view, as its X. | - | X of the title strip | OOS extension |
| `:DOCK:ACTivate` | Set | `<view>`: brings its tab to the front of its group. | - | Tap a tab | OOS extension |
| `:DOCK:MAXimise` | Set / Query | `<view>`: every view in tabs of one group that fills the area, `<view>` in front. The query answers `1` while a view fills the area. | - | Expand button; right-click > Maximise view | OOS extension |
| `:DOCK:RESTore` | Set | Back from a maximised view to the arrangement before. | - | Restore button; right-click > Restore layout | OOS extension |
| `:DOCK:RESet` | Set | The default arrangement: nothing floating, no saved splits. | - | - | OOS extension |
| `:DOCK:LAYout` | Set / Query | The whole arrangement as a quoted text: the setup's text (`Model::studio_dock`) with `|` between its parts, since `;` would end the command. Read it, and set it back later to restore an arrangement. | - | - | OOS extension |

### Right-click menu of the plot

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:VIEW:TRACes` | Set / Query | `OVERlay\|STACked`: all traces in one plot, or one lane each. | `OVER` | Right-click > Overlay traces, Stacked traces | OOS extension |
| `:VIEW:FIT` | Set | Fits the whole record into the screen. | - | Right-click > Fit record | OOS extension |
| `:ZOOM:HERE` | Set / Query | Time in seconds: opens the zoom window there (or moves it). The query answers the zoom window's centre. | - | Right-click > Zoom here | OOS extension |
| `:ZOOM:BOX` | Set / Query | `<t0>,<t1>` seconds: the finest zoom step that holds the span, centred on it. The query answers the zoom window as `<t0>,<t1>`, `0,0` when zoom is off. | - | Right-click > Zoom box | OOS extension |
| `:ZOOM:RESet` | Set | The zoom window back to its default size and place. | - | Right-click > Reset zoom | OOS extension |
| `:ZOOM:CLOSe` | Set | Closes the zoom window. | - | Right-click > Close zoom | OOS extension |

The other rows of that menu have their commands already: Stop / Run `:STOP`, `:RUN`; Single `:SINGle`; Autoset `:AUToscale`; Trigger on a channel `:TRIGger:EDGE:SOURce`; Set level here `:TRIGger:EDGE:LEVel`; Measurement on a channel `:MEASure:ITEM`; Cursor here `:CURSor:...`. Any row can also be pressed by name with `:UI:CONText` and `:UI:MENU:SELect`.

### Screen controls

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:UI:SCReen` | Query | Answers `w,h`, the screen size in pixels. | - | - | OOS extension |
| `:UI:ELEMents` | Query | `[<kind>]`: every control on the screen now, one record each, separated by `;`: `<kind>,"<label>",x,y,w,h`. Kinds: `TOP` (top bar buttons), `BOTTOM` (channel, reference and math chips, time and trigger), `VIEW` (a view, label `Name (WAVE)`), `CLOSE`, `EXPAND`, `TAB`, `RESIZE` (a floating panel's corner), `DIVIDER` (the hit area of a divider), `BADGE`, `CURSOR`, `MENU` (rows of the menu that is open). With a kind, only those. | - | - | OOS extension |
| `:UI:TAP` | Set | `x,y`: a tap, as a finger. | - | - | OOS extension |
| `:UI:DRAG` | Set | `x0,y0,x1,y1`: a finger down, moved in steps, up. Drags a divider, a view by its strip, a floating panel by its corner, a cursor. | - | - | OOS extension |
| `:UI:CONText` | Set | `x,y`: what a right click or a held finger opens at that point: the plot's menu, or the menu of a channel chip or a view. | - | Right click; long press | OOS extension |
| `:UI:MENU` | Query | The menu or popup that is open: its title, then each row, as quoted strings separated by commas. A side menu lists its items as `"<label> = <value>"`. `""` when none is open. | - | - | OOS extension |
| `:UI:MENU:SELect` | Set | `"<label>"`: presses the row of the open menu with that label (case does not matter; a unique beginning is enough). | - | Tap a menu row | OOS extension |
| `:UI:MENU:CLOSe` | Set | Closes whatever menu or popup is open. | - | Tap outside | OOS extension |

## Examples

The measurement table to the right of the waveform, a third of the width:

```
:DOCK:MOVE MEAS,WAVE,RIGHT
:DOCK:SIZE MEAS,0.25
:DOCK:LIST?
WAVE,DOCK,0,40,765,524;MEAS,DOCK,769,40,255,524;EYE,OFF,0,0,0,0;PANE,OFF,0,0,0,0;...
```

(Replies of a DHO924S, screen 1024 x 600.)

The spectrum pane floating in the top-right corner, then back under the waveform:

```
:DOCK:FLOAT PANE,820,90,440,280
:DOCK:MOVE PANE,WAVE,BOTTOM
```

Save an arrangement and come back to it:

```
:DOCK:LAYout?
"V0.246(M,W)|M@311:77:337:280"
:DOCK:LAYout "V0.246(M,W)|M@311:77:337:280"
```

Zoom on 10 ms around the trigger, read the window, then close the zoom:

```
:ZOOM:BOX -5e-3,5e-3
:ZOOM:BOX?
-5.000000E-03,5.000000E-03
:ZOOM:CLOSe
```

Right-click the plot and pick a row by its label, as a person would:

```
:UI:CONText 600,300
:UI:MENU?
"Context","Overlay traces","Stacked traces","Fit record","Zoom here","Zoom box","Stop","Single","Autoset","Trigger on CH2","Set level here","Trigger settings","Measurement on CH2","Cursor here","Sources","Maximise view"
:UI:MENU:SELect "Stacked traces"
```

Find a button on the screen and press it:

```
:UI:ELEMents? TOP
TOP,"Views",601,5,80,30;TOP,"Add",687,5,53,30;TOP,"SIM",164,5,52,30;TOP,"Stop",8,6,92,28;TOP,"Single",104,5,54,30;...
:UI:TAP 131,20
```
