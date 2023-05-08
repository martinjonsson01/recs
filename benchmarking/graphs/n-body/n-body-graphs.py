import zipfile
import pandas as pd
import matplotlib.pyplot as plt
import seaborn as sns
import re
from pathlib import Path

def format_name(s):
    s = re.sub(r'entt', 'EnTT', s)
    s = re.sub(r'linux', 'Linux', s)
    s = re.sub(r'\.csv', '', s)
    s = re.sub(r'win', 'Windows', s)
    s = re.sub(r'unity', 'Unity', s)
    s = re.sub(r'-', ' ', s)
    s = re.sub(r'gameobjects', 'GameObjects', s)
    s = re.sub(r'clang', 'Clang', s)
    s = re.sub(r'gcc', 'GCC', s)
    s = re.sub(r'msvc', 'MSVC', s)
    s = re.sub(r'icx', 'ICX', s)
    s = re.sub(r'mono', 'Mono', s)
    s = re.sub(r'il2cpp', 'IL2CPP', s)
    s = re.sub(r'n_body_recs', 'RECS Linux', s)
    s = re.sub(r'n_body_bevy', 'Bevy Linux', s)
    s = re.sub(r'ecs', 'ECS', s)
    return s

def save_graph(fig, file_name):
    output_path = Path("output")
    output_path.mkdir(parents=True, exist_ok=True)
    fig.savefig(f'{output_path}/{file_name}.svg')
    plt.close(fig)

def plot_line_graph(df, engine, combination, ax):
    if not any(name in engine["name"] for name in combination["engine"]):
        return

    linestyle, linewidth = get_linestyle_and_linewidth(engine['name'])
    ax.plot(df['body_count'], df['sample_mean'], label=engine['name'], color=engine['color'], linestyle=linestyle, linewidth=linewidth)


def create_line_graphs(engines):
    combinations = create_engine_combinations_for_linegraphs()

    for combination in combinations:
        fig, ax = plt.subplots(figsize=(10, 8))
        # Plot the data for each engine
        for engine in engines:
            plot_line_graph(engine['df'], engine, combination, ax)

        # Adjust Graph

        #Set scales
        ax.set_xscale('log', base=2, )
        ax.set_yscale('log')
        ax.set_xticks([2 ** i for i in range(0, 15)])

        # Set lables and title
        ax.set_xlabel('Number of bodies')
        ax.set_ylabel('Time (seconds)')
        ax.set_title(f'Average Tick Duration for N-body Simulation ({combination["name"]})')

        # Add Grid
        ax.set_axisbelow(True)
        ax.grid(color='gray', linestyle='solid')

        ax.legend()
        file_name = combination["name"].replace(' ', '').replace(',', '-')
        save_graph(fig, file_name)

def get_linestyle_and_linewidth(name):
    if name == 'RECS Linux':
        return 'dotted', 5
    else:
        return 'solid', 1.5

def create_engine_combinations_for_linegraphs():
    combinations = []
    combinations.append({"engine": ["Unity"], "name":'Unity, All OSes, All compilers'})
    combinations.append({"engine": ['EnTT'], "name":'EnTT, All OSes, All compilers'})
    combinations.append({"engine": ['RECS', 'Bevy', 'ICX', 'Unity ECS Linux', 'Linux IL2CPP'], "name":'All engines, Best OSes, Best compilers'})
    combinations.append({"engine": ['RECS', 'Bevy', 'ICX', 'Unity ECS Linux'], "name":'ECS engines, Best OSes, Best compilers'})
    combinations.append({"engine": ['RECS', 'Bevy', 'Unity ECS', 'EnTT'], "name":'ECS engines, All OSes, All compilers'})
    combinations.append({"engine": [' '], "name":'All Engines, All OSes, All compilers'})
    return combinations

def format_number(num):
    if num >= 0.1:
        return f"{num:.2f} s"
    else:
        return f"{num*1e3:.2f} ms"

def get_suffix_from_number(num):
    if num >= 0.1:
        return "seconds"
    else:
        return "milliseconds"


def get_data_for_bar_graph(engine, nr_of_bodies, data):
    for name in ['RECS', 'Bevy', 'Unity ECS', 'EnTT']:
        if name in engine["name"]:
            df = engine['df']
            time = df.loc[df['body_count'] == nr_of_bodies, 'sample_mean'].values[0].astype(float)
            error = df.loc[df['body_count'] == nr_of_bodies, 'sample_error'].values[0].astype(float)
            color = engine['color']
            data.append((time, error, engine["name"], color))
            break

def create_bar_graphs(engines, nr_of_bodies_list=[16384, 1024]):
    for nr_of_bodies in nr_of_bodies_list:
        # Get data for each engine
        data = []
        for engine in engines:
            get_data_for_bar_graph(engine, nr_of_bodies, data)


        # Sort data by time
        data = sorted(data, reverse=True)

        # Create figure and axis objects
        fig, ax = plt.subplots(figsize=(10, 8))

        # Add bars for each engine
        for i, (time, error, name, color) in enumerate(data):
            ax.barh(i, time, color=color, label=name, xerr=error, height=0.8, error_kw={'linewidth': 0.8, 'capsize': 8, 'alpha': 1})
            bar_value = format_number(time)
            ax.text(time - 0.1 * time, i, bar_value, ha='right', va='center', color='black', fontsize=10)

        # Set axis labels and title
        ax.set_yticks(range(len(data)))
        ax.set_yticklabels([name for _, _, name, _ in data], fontsize=10)
        ax.invert_yaxis()
        ax.set_ylabel('Engines')
        ax.set_xlabel(f'Time ({get_suffix_from_number(data[0][0])})')
        ax.set_title(f'Average Tick Duration for N-body Simulation for {nr_of_bodies} Bodies')
        ax.set_axisbelow(True)
        ax.xaxis.grid(which='major', linestyle='solid', color='gray', alpha=1)

        # Adjust layout and save figure
        plt.subplots_adjust(left=0.2)
        file_name = f'bar-graph-best-ecs-{nr_of_bodies}'
        save_graph(fig, file_name)

if __name__ == '__main__':
    with zipfile.ZipFile('n-body-results.zip') as myzip:
        files = myzip.namelist()
        csv_files = [f for f in files if f.endswith('.csv')]
        colors = sns.color_palette('colorblind', n_colors=len(csv_files)).as_hex()
        engines = []

        for file in csv_files:
            with myzip.open(file) as csvfile:
                engine = {}
                # Adds the name and color of the engine
                engine['name'] = format_name(file)
                engine['color'] = colors.pop()
                df = pd.read_csv(csvfile)

                # Fetches all the sample columns
                sample_cols = df.filter(regex=r'sample_')

                # Apply the str.replace() and astype() functions to each column
                sample_cols = sample_cols.apply(lambda x: x.str.replace(',', '.').astype(float))

                # Replace the original columns in the dataframe with the updated ones
                df[sample_cols.columns] = sample_cols

                # Calculate the mean and error
                sample_mean = sample_cols.mean(axis=1)
                sample_error = sample_cols.std(axis=1, ddof=1)

                # Add the mean collumn
                df['sample_mean'] = sample_mean
                # Add the error collumn
                df['sample_error'] = sample_error

                engine['df'] = df
                engines.append(engine)

        plt.rcParams.update({'font.size': 13})
        create_line_graphs(engines)
        create_bar_graphs(engines)
