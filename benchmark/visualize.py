#!/usr/bin/env python3
"""
Performance benchmark visualization for miwidothttp vs nginx
Generates comparison charts from benchmark results
"""

import os
import re
import sys
import glob
import pandas as pd
import matplotlib.pyplot as plt
import seaborn as sns
from datetime import datetime

def parse_benchmark_file(filepath):
    """Parse a single benchmark result file"""
    with open(filepath, 'r') as f:
        content = f.read()
    
    metrics = {}
    
    # Extract requests/sec
    req_match = re.search(r'Requests/sec:\s+([\d.]+)', content)
    if req_match:
        metrics['requests_per_sec'] = float(req_match.group(1))
    
    # Extract transfer rate (KB/sec)
    transfer_match = re.search(r'Bytes/sec:\s+([\d.]+)', content)
    if transfer_match:
        metrics['transfer_rate_kb'] = float(transfer_match.group(1))
    
    # Extract latencies
    latency_50 = re.search(r'50%:\s+([\d.]+)\s*ms', content)
    latency_75 = re.search(r'75%:\s+([\d.]+)\s*ms', content)
    latency_90 = re.search(r'90%:\s+([\d.]+)\s*ms', content)
    latency_99 = re.search(r'99%:\s+([\d.]+)\s*ms', content)
    latency_999 = re.search(r'99\.9%:\s+([\d.]+)\s*ms', content)
    
    if latency_50:
        metrics['latency_p50'] = float(latency_50.group(1))
    if latency_75:
        metrics['latency_p75'] = float(latency_75.group(1))
    if latency_90:
        metrics['latency_p90'] = float(latency_90.group(1))
    if latency_99:
        metrics['latency_p99'] = float(latency_99.group(1))
    if latency_999:
        metrics['latency_p999'] = float(latency_999.group(1))
    
    # Extract errors
    errors_match = re.search(r'Errors:\s+(\d+)', content)
    if errors_match:
        metrics['errors'] = int(errors_match.group(1))
    
    return metrics

def load_benchmark_results(results_dir):
    """Load all benchmark results from a directory"""
    data = []
    
    for filepath in glob.glob(os.path.join(results_dir, '*.txt')):
        filename = os.path.basename(filepath)
        if filename == 'summary.txt':
            continue
            
        parts = filename.replace('.txt', '').split('_')
        if len(parts) >= 2:
            server = parts[0]
            test_name = '_'.join(parts[1:])
            
            metrics = parse_benchmark_file(filepath)
            if metrics:
                metrics['server'] = server
                metrics['test'] = test_name
                data.append(metrics)
    
    return pd.DataFrame(data)

def create_performance_matrix(df, output_dir):
    """Create performance comparison matrix"""
    fig, axes = plt.subplots(2, 3, figsize=(15, 10))
    fig.suptitle('Performance Comparison: miwidothttp vs nginx', fontsize=16)
    
    # Requests per second comparison
    ax = axes[0, 0]
    pivot_rps = df.pivot(index='test', columns='server', values='requests_per_sec')
    pivot_rps.plot(kind='bar', ax=ax)
    ax.set_title('Requests per Second')
    ax.set_ylabel('Requests/sec')
    ax.set_xlabel('Test')
    ax.legend(title='Server')
    ax.grid(True, alpha=0.3)
    
    # Transfer rate comparison
    ax = axes[0, 1]
    if 'transfer_rate_kb' in df.columns:
        pivot_transfer = df.pivot(index='test', columns='server', values='transfer_rate_kb')
        pivot_transfer.plot(kind='bar', ax=ax)
        ax.set_title('Transfer Rate')
        ax.set_ylabel('KB/sec')
        ax.set_xlabel('Test')
        ax.legend(title='Server')
        ax.grid(True, alpha=0.3)
    
    # Latency percentiles
    ax = axes[0, 2]
    latency_cols = ['latency_p50', 'latency_p90', 'latency_p99']
    miwi_data = df[df['server'] == 'miwidothttp'][latency_cols].mean()
    nginx_data = df[df['server'] == 'nginx'][latency_cols].mean()
    
    x = range(len(latency_cols))
    width = 0.35
    ax.bar([i - width/2 for i in x], miwi_data, width, label='miwidothttp')
    ax.bar([i + width/2 for i in x], nginx_data, width, label='nginx')
    ax.set_title('Average Latency Percentiles')
    ax.set_ylabel('Latency (ms)')
    ax.set_xticks(x)
    ax.set_xticklabels(['p50', 'p90', 'p99'])
    ax.legend()
    ax.grid(True, alpha=0.3)
    
    # Performance ratio (miwidothttp/nginx)
    ax = axes[1, 0]
    if len(pivot_rps.columns) == 2:
        ratio = pivot_rps['miwidothttp'] / pivot_rps['nginx']
        ratio.plot(kind='bar', ax=ax, color=['green' if r >= 1 else 'red' for r in ratio])
        ax.set_title('Performance Ratio (miwidothttp/nginx)')
        ax.set_ylabel('Ratio')
        ax.set_xlabel('Test')
        ax.axhline(y=1, color='black', linestyle='--', alpha=0.5)
        ax.grid(True, alpha=0.3)
    
    # Latency distribution
    ax = axes[1, 1]
    latency_data = []
    for server in df['server'].unique():
        server_df = df[df['server'] == server]
        for col in ['latency_p50', 'latency_p75', 'latency_p90', 'latency_p99']:
            if col in server_df.columns:
                for val in server_df[col]:
                    latency_data.append({
                        'server': server,
                        'percentile': col.replace('latency_', ''),
                        'latency': val
                    })
    
    if latency_data:
        lat_df = pd.DataFrame(latency_data)
        sns.boxplot(data=lat_df, x='percentile', y='latency', hue='server', ax=ax)
        ax.set_title('Latency Distribution')
        ax.set_ylabel('Latency (ms)')
        ax.set_xlabel('Percentile')
        ax.grid(True, alpha=0.3)
    
    # Summary statistics
    ax = axes[1, 2]
    summary_text = "Summary Statistics\n" + "="*30 + "\n\n"
    
    for server in df['server'].unique():
        server_df = df[df['server'] == server]
        summary_text += f"{server.upper()}:\n"
        summary_text += f"  Avg RPS: {server_df['requests_per_sec'].mean():.0f}\n"
        if 'latency_p50' in server_df.columns:
            summary_text += f"  Avg p50: {server_df['latency_p50'].mean():.2f}ms\n"
        if 'latency_p99' in server_df.columns:
            summary_text += f"  Avg p99: {server_df['latency_p99'].mean():.2f}ms\n"
        if 'errors' in server_df.columns:
            summary_text += f"  Total Errors: {server_df['errors'].sum()}\n"
        summary_text += "\n"
    
    ax.text(0.1, 0.5, summary_text, transform=ax.transAxes, fontsize=10,
            verticalalignment='center', fontfamily='monospace')
    ax.axis('off')
    
    plt.tight_layout()
    
    # Save the figure
    output_file = os.path.join(output_dir, 'performance_matrix.png')
    plt.savefig(output_file, dpi=150, bbox_inches='tight')
    print(f"Performance matrix saved to: {output_file}")
    
    return fig

def generate_report(df, output_dir):
    """Generate a detailed HTML report"""
    html_content = f"""
    <!DOCTYPE html>
    <html>
    <head>
        <title>Benchmark Report - miwidothttp vs nginx</title>
        <style>
            body {{ font-family: Arial, sans-serif; margin: 40px; }}
            h1 {{ color: #333; }}
            table {{ border-collapse: collapse; width: 100%; margin: 20px 0; }}
            th, td {{ border: 1px solid #ddd; padding: 12px; text-align: left; }}
            th {{ background-color: #4CAF50; color: white; }}
            tr:nth-child(even) {{ background-color: #f2f2f2; }}
            .winner {{ background-color: #d4edda; font-weight: bold; }}
            .summary {{ background-color: #f8f9fa; padding: 20px; border-radius: 5px; margin: 20px 0; }}
            img {{ max-width: 100%; height: auto; }}
        </style>
    </head>
    <body>
        <h1>Performance Benchmark Report</h1>
        <p>Generated: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}</p>
        
        <div class="summary">
            <h2>Executive Summary</h2>
            <ul>
    """
    
    # Calculate overall performance
    miwi_avg_rps = df[df['server'] == 'miwidothttp']['requests_per_sec'].mean()
    nginx_avg_rps = df[df['server'] == 'nginx']['requests_per_sec'].mean()
    performance_ratio = (miwi_avg_rps / nginx_avg_rps - 1) * 100
    
    html_content += f"""
                <li>Average Requests/sec - miwidothttp: {miwi_avg_rps:.0f}, nginx: {nginx_avg_rps:.0f}</li>
                <li>Performance Difference: {performance_ratio:+.1f}%</li>
    """
    
    if 'latency_p50' in df.columns:
        miwi_avg_p50 = df[df['server'] == 'miwidothttp']['latency_p50'].mean()
        nginx_avg_p50 = df[df['server'] == 'nginx']['latency_p50'].mean()
        html_content += f"""
                <li>Average p50 Latency - miwidothttp: {miwi_avg_p50:.2f}ms, nginx: {nginx_avg_p50:.2f}ms</li>
        """
    
    html_content += """
            </ul>
        </div>
        
        <h2>Detailed Results</h2>
        <table>
            <tr>
                <th>Test</th>
                <th>Server</th>
                <th>Requests/sec</th>
                <th>p50 Latency (ms)</th>
                <th>p90 Latency (ms)</th>
                <th>p99 Latency (ms)</th>
                <th>Errors</th>
            </tr>
    """
    
    # Add table rows
    for test in df['test'].unique():
        for server in ['miwidothttp', 'nginx']:
            row = df[(df['test'] == test) & (df['server'] == server)]
            if not row.empty:
                row = row.iloc[0]
                html_content += f"""
            <tr class="{'winner' if server == 'miwidothttp' and row['requests_per_sec'] > df[(df['test'] == test) & (df['server'] == 'nginx')]['requests_per_sec'].values[0] else ''}">
                <td>{test}</td>
                <td>{server}</td>
                <td>{row.get('requests_per_sec', 'N/A'):.0f}</td>
                <td>{row.get('latency_p50', 'N/A'):.2f}</td>
                <td>{row.get('latency_p90', 'N/A'):.2f}</td>
                <td>{row.get('latency_p99', 'N/A'):.2f}</td>
                <td>{row.get('errors', 0)}</td>
            </tr>
                """
    
    html_content += """
        </table>
        
        <h2>Performance Visualization</h2>
        <img src="performance_matrix.png" alt="Performance Matrix">
        
    </body>
    </html>
    """
    
    report_file = os.path.join(output_dir, 'report.html')
    with open(report_file, 'w') as f:
        f.write(html_content)
    
    print(f"HTML report saved to: {report_file}")

def main():
    if len(sys.argv) < 2:
        print("Usage: python visualize.py <results_directory>")
        sys.exit(1)
    
    results_dir = sys.argv[1]
    
    if not os.path.exists(results_dir):
        print(f"Error: Results directory '{results_dir}' does not exist")
        sys.exit(1)
    
    print(f"Loading benchmark results from: {results_dir}")
    df = load_benchmark_results(results_dir)
    
    if df.empty:
        print("No benchmark results found")
        sys.exit(1)
    
    print(f"Loaded {len(df)} benchmark results")
    
    # Create visualizations
    create_performance_matrix(df, results_dir)
    
    # Generate HTML report
    generate_report(df, results_dir)
    
    print("\nVisualization complete!")

if __name__ == "__main__":
    main()