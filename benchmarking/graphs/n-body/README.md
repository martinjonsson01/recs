# How to Run N-body Simulation Graph Creator

This code provides tools to generate graphs based on the results of the N-body simulation benchmarks.

## Requirements

1. Python 3.6+
1. pandas
1. matplotlib
1. seaborn
1. regex

## How to run the tool
1. Make sure Python 3.6 or later is installed
1. Use `pip install` to install the different libraires, if they are not installed already
2. Place the zip file named `n-body-results.zip`, containing the CSV files for the benchmarks, in the same folder as `n-body-graphs.py`
1. Run the tool. Open a command prompt/terminal in the folder containing `n-body-graphs.py` and type the following command: `python n-body-graphs.py`
1. The tool will create a folder named `output` containing the different line and bar graphs as SVGs.