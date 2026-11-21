Fixed:

- The eye page no longer takes all the memory of the scope when a bit rate from a faster time base is kept at a slow
  one: a rate the record has under two samples a bit for is found again from the record when it was found from one,
  and a rate that was set stays and the page says that it needs a faster time base. The clock recovery takes no
  bit shorter than half a sample, which bounds the eye, the jitter figures and the TIE trend for every caller. Seen
  on a unit: the program was stopped by the kernel at 3.7 GB
  ([Measurements and Analysers](wiki/Measurements-and-Analysers.md)).
