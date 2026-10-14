"""usage: python3 -I progress.py header 'phase' 'cell' 'last result' 'problems' 'next'
       python3 -I progress.py log 'message'"""
import os, sys, re, datetime
P = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "results", "PROGRESS.md")
now = datetime.datetime.now()
text = open(P).read() if os.path.exists(P) else "Phase: none\n\nLog:\n"
if sys.argv[1] == "log":
    text = text.rstrip("\n") + f"\n- {now:%Y-%m-%d %H:%M} {sys.argv[2]}\n"
else:
    phase, cell, last, prob, nxt = sys.argv[2:7]
    m = re.search(r"Started: (\S+ \S+)", text)
    started = m.group(1) if m else f"{now:%Y-%m-%d %H:%M}"
    head = (f"Phase: {phase}\nCell: {cell}\nStarted: {started}   Last update: {now:%H:%M}\n"
            f"Last result: {last}\nProblems: {prob}\nNext: {nxt}\n")
    text = re.sub(r"\A(?:Phase|Cell|Started|Last result|Problems|Next):.*\n(?:(?:Phase|Cell|Started|Last result|Problems|Next):.*\n)*", head, text)
open(P, "w").write(text)
