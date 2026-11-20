Fixed:

- On the live acquisition, Run after a Single (`:RUN` after `:SINGle`, or Run/Stop after the Single key) goes back to
  the sweep before the Single, Auto or Normal; it went to Auto, so a Normal sweep was lost after every Single. A stored
  replay and SIM still go to Normal ([SCPI and VXI-11](wiki/SCPI-and-VXI-11.md)).
