Fixed:

- The web page `/update` shows "no answer within 10 s, stopped" when `oos-update status` hangs, instead of waiting for
  it and keeping the web server's threads for large answers busy.
