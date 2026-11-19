Fixed:

- A CSV file loaded into a reference takes the time between its samples over its whole time column, not from its first
  two lines: a file that writes its times with a few digits read every frequency up to a percent off, and one whose
  first two lines have the same rounded time was refused. In a file of several sweeps the longest one gives the step
  ([First Steps](wiki/First-Steps.md#saving)).
