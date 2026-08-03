#!/usr/bin/env python3
"""Plot the metrics CSV produced by Genesis.

Usage:
    python3 tools/plot_stats.py data/stats.csv [output.png]

Reads the CSV (written by `genesis::export` / examples/observe_quench.rs) and
plots, over simulated time:
    - total / kinetic / potential energy
    - average temperature
    - number of aggregates and the largest one (emergent structure)
    - simulation speed (ticks/s) and memory

Without matplotlib installed, prints a text summary instead.
"""

import csv
import sys


def load(path):
    with open(path, newline="") as f:
        return list(csv.DictReader(f))


def summary(rows):
    first, last = rows[0], rows[-1]
    print(f"ticks: {first['tick']} -> {last['tick']} "
          f"(t={first['time_s']}s -> {last['time_s']}s)")
    print(f"T_avg : {float(first['temperature_avg']):8.2f} K -> "
          f"{float(last['temperature_avg']):8.2f} K")
    print(f"E_tot : {float(first['energy_total']):10.3f} -> "
          f"{float(last['energy_total']):10.3f}")
    print(f"largest aggregate: {max(int(r['largest']) for r in rows)} "
          f"(of {int(last['entities'])} atoms)")
    print(f"peak speed: {max(float(r['fps']) for r in rows):.0f} ticks/s "
          f"(end: {float(last['fps']):.0f})")


def plot(rows, out):
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt

    t = [float(r["time_s"]) for r in rows]
    fig, axes = plt.subplots(4, 1, figsize=(9, 14), sharex=True)

    axes[0].plot(t, [float(r["energy_total"]) for r in rows], label="E total")
    axes[0].plot(t, [float(r["energy_kinetic"]) for r in rows], label="K")
    axes[0].plot(t, [float(r["energy_potential"]) for r in rows], label="V")
    axes[0].set_ylabel("energy"); axes[0].legend(); axes[0].grid(alpha=0.3)

    axes[1].plot(t, [float(r["temperature_avg"]) for r in rows], color="tab:red")
    axes[1].set_ylabel("T (K)"); axes[1].grid(alpha=0.3)

    axes[2].plot(t, [int(r["aggregates"]) for r in rows], label="aggregates")
    axes[2].plot(t, [int(r["largest"]) for r in rows], label="largest")
    axes[2].set_ylabel("clusters"); axes[2].legend(); axes[2].grid(alpha=0.3)

    axes[3].plot(t, [float(r["fps"]) for r in rows], color="tab:green")
    axes[3].set_xlabel("time (s)"); axes[3].set_ylabel("ticks/s")
    axes[3].grid(alpha=0.3)

    fig.tight_layout()
    fig.savefig(out, dpi=150)
    print(f"plot written to {out}")


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(1)
    rows = load(sys.argv[1])
    if not rows:
        print("empty CSV")
        sys.exit(1)
    summary(rows)
    out = sys.argv[2] if len(sys.argv) > 2 else "stats.png"
    try:
        plot(rows, out)
    except ImportError:
        print("(matplotlib not installed: printed text summary only)")


if __name__ == "__main__":
    main()
