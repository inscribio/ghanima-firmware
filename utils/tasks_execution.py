#!/usr/bin/env python

import argparse
import pandas as pd

def main(args=None):
    parser = argparse.ArgumentParser(description='''
        Analyze tasks execution based on data from logic analyzer.
        Assumes that the measurements file is in CSV format with a header row.
    ''')
    parser.add_argument('measurements', help='Logic analyzer measurements in CSV format')
    parser.add_argument('--time-col', type=int, default=0,
                        help='Index of data column containing time values in seconds')
    parser.add_argument('--task-col', type=int, default=1,
                        help='Index of data column containing values of debug::tasks::task pin')
    parser.add_argument('--trace-col', type=int, default=2,
                        help='Index of data column containing values of debug::tasks::trace pin')
    parser.add_argument(
        '--trace-groups', type=int, default=5,
        help='Number of bins to use when generating histogram of trace pin pulse durations')
    parser.add_argument('--trace-decimals', type=int, default=5,
                        help='Number of decimal places for calculating trace stats by rounding')
    args = parser.parse_args(args)

    cols = [args.time_col, args.task_col, args.trace_col]
    assert len(set(cols)) == len(cols), f'Duplicate columns: {cols}'

    df = pd.read_csv(args.measurements)
    df.rename(columns={
        df.columns[args.time_col]: 'start',
        df.columns[args.task_col]: 'tasks',
        df.columns[args.trace_col]: 'trace',
    }, inplace=True)

    # Start from time 0
    df['start'] -= df.loc[0, 'start']

    # Calculate durations
    df['end'] = df['start'].shift(-1)
    df['duration'] = (df['end'] - df['start']).fillna(0)

    # Tasks
    run_time = df.loc[df['tasks'] == 1, 'duration'].sum()
    idle_time = df.loc[df['tasks'] == 0, 'duration'].sum()
    print('Tasks:')
    print(f'  Idle time = {idle_time * 1e3:.3f} ms')
    print(f'  Run time  = {run_time * 1e3:.3f} ms')
    print(f'  CPU usage = {run_time / (run_time + idle_time) * 100:.1f}%')

    # Trace

    # Merge sequences of same values into single rows:
    # Compare with shifted: True when a new value appears after a sequence of same values
    df['new_trace'] = df['trace'] != df['trace'].shift()
    # Apply consecutive group numbers to each changed trace value
    df['groups'] = df['new_trace'].cumsum()
    # Sum durations for each group
    durations = df.groupby(df['groups'])['duration'].sum()
    # Create a new data frame without the duplicates
    merged = df[df['new_trace']].copy()
    # Reset the index to properly assign durations
    merged.reset_index(drop=True, inplace=True)
    merged['duration'] = durations.reset_index(drop=True)
    # Remove unneeded columns
    merged.drop(columns=['end', 'tasks', 'new_trace', 'groups'], inplace=True)

    # Get durations of periods where the pin was high
    trace = pd.DataFrame()
    trace['duration'] = merged[merged['trace'] == 1]['duration'].reset_index(drop=True)
    trace['range'] = pd.cut(trace['duration'], args.trace_groups)
    stats = trace.groupby('range').aggregate(func=['mean', 'std', 'count', 'sum'])
    print('\nTrace statistics (pandas.cut):')
    print(stats)

    # Different method
    trace.drop(columns='range', inplace=True)
    trace['approx'] = trace['duration'].round(decimals=args.trace_decimals)
    stats = trace.groupby('approx').aggregate(func=['mean', 'std', 'count', 'sum'])
    print('\nTrace statistics (round to decimals):')
    print(stats)


if __name__ == "__main__":
    main()
