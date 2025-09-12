#!/usr/bin/env python3
"""
Execution CSV Comparator Tool

Compares two execution consistency CSV files from separate runs (embedded vs remote)
and reports differences in transaction execution results.

Usage:
    python3 execution_csv_compare.py --left embedded.csv --right remote.csv --output reports/
"""

import argparse
import csv
import json
import os
import sys
from collections import defaultdict
from typing import Dict, List, Tuple, Optional, Any


class ExecutionComparator:
    """Main class for comparing execution CSV files."""
    
    # Fields to compare by default
    COMPARISON_FIELDS = [
        'is_success',
        'result_code', 
        'energy_used',
        'return_data_hex',
        'runtime_error',
        'state_digest_sha256',
        'state_change_count'
    ]
    
    # Fields that should be exact matches
    EXACT_FIELDS = {
        'is_success',
        'result_code',
        'return_data_hex',
        'runtime_error',
        'state_digest_sha256'
    }
    
    # Fields that allow small numeric differences (for future extensions)
    NUMERIC_FIELDS = {
        'energy_used',
        'state_change_count'
    }
    
    def __init__(self, left_file: str, right_file: str, output_dir: str, 
                 comparison_fields: List[str] = None, ignore_return_data: bool = False):
        self.left_file = left_file
        self.right_file = right_file
        self.output_dir = output_dir
        self.ignore_return_data = ignore_return_data
        
        # Set comparison fields
        if comparison_fields:
            self.comparison_fields = comparison_fields
        else:
            self.comparison_fields = self.COMPARISON_FIELDS.copy()
            
        if ignore_return_data:
            self.comparison_fields = [f for f in self.comparison_fields if f != 'return_data_hex']
        
        # Statistics
        self.stats = {
            'left_total': 0,
            'right_total': 0,
            'matched': 0,
            'left_only': 0,
            'right_only': 0,
            'field_mismatches': defaultdict(int),
            'total_field_comparisons': defaultdict(int)
        }
        
        # Results
        self.mismatches = []
        
    def load_csv_with_index(self, filename: str) -> Tuple[Dict[str, Dict], Dict[Tuple, Dict]]:
        """
        Load CSV file and create indexes by tx_id_hex and (block_num, tx_index_in_block).
        
        Returns:
            Tuple of (tx_id_index, block_tx_index)
        """
        tx_id_index = {}
        block_tx_index = {}
        row_count = 0
        
        print(f"Loading {filename}...")
        
        try:
            with open(filename, 'r', encoding='utf-8') as f:
                reader = csv.DictReader(f)
                for row in reader:
                    row_count += 1
                    
                    # Primary index: tx_id_hex
                    tx_id = row.get('tx_id_hex', '').strip()
                    if tx_id:
                        if tx_id in tx_id_index:
                            print(f"Warning: Duplicate tx_id_hex {tx_id} in {filename}")
                        tx_id_index[tx_id] = row
                    
                    # Fallback index: (block_num, tx_index_in_block)
                    try:
                        block_num = int(row.get('block_num', 0))
                        tx_index = int(row.get('tx_index_in_block', 0))
                        block_tx_key = (block_num, tx_index)
                        if block_tx_key in block_tx_index:
                            print(f"Warning: Duplicate (block_num, tx_index) {block_tx_key} in {filename}")
                        block_tx_index[block_tx_key] = row
                    except (ValueError, TypeError):
                        print(f"Warning: Invalid block_num/tx_index in row {row_count} of {filename}")
                        
        except FileNotFoundError:
            print(f"Error: File {filename} not found")
            sys.exit(1)
        except Exception as e:
            print(f"Error loading {filename}: {e}")
            sys.exit(1)
            
        print(f"Loaded {row_count} rows from {filename}")
        return tx_id_index, block_tx_index
    
    def normalize_value(self, field: str, value: str) -> Any:
        """Normalize field values for comparison."""
        if not value:
            return ""
            
        # Handle boolean fields
        if field == 'is_success':
            return value.lower().strip() in ('true', '1', 'yes')
            
        # Handle numeric fields
        if field in self.NUMERIC_FIELDS:
            try:
                return int(value)
            except ValueError:
                return 0
                
        # Handle hex fields - normalize to lowercase
        if field in ('return_data_hex', 'state_digest_sha256', 'tx_id_hex', 'block_id_hex'):
            return value.lower().strip()
            
        # Default: trim whitespace
        return value.strip()
    
    def compare_field(self, field: str, left_val: Any, right_val: Any) -> bool:
        """Compare two field values, returning True if they match."""
        if field in self.EXACT_FIELDS:
            return left_val == right_val
        elif field in self.NUMERIC_FIELDS:
            # For numeric fields, allow exact match for now
            # Future: could add tolerance for energy_used
            return left_val == right_val
        else:
            return left_val == right_val
    
    def join_and_compare(self, left_tx_idx: Dict, left_block_idx: Dict,
                        right_tx_idx: Dict, right_block_idx: Dict) -> None:
        """Join records and perform field-by-field comparison."""
        
        print("Performing join and comparison...")
        
        # Track all transaction IDs from both sides
        all_tx_ids = set(left_tx_idx.keys()) | set(right_tx_idx.keys())
        all_block_keys = set(left_block_idx.keys()) | set(right_block_idx.keys())
        
        self.stats['left_total'] = len(left_tx_idx)
        self.stats['right_total'] = len(right_tx_idx)
        
        # Primary join: by tx_id_hex
        for tx_id in all_tx_ids:
            left_row = left_tx_idx.get(tx_id)
            right_row = right_tx_idx.get(tx_id)
            
            if left_row and right_row:
                self._compare_rows(left_row, right_row, 'tx_id_hex', tx_id)
            elif left_row and not right_row:
                self.stats['left_only'] += 1
                self._record_missing('right', left_row, 'tx_id_hex', tx_id)
            elif right_row and not left_row:
                self.stats['right_only'] += 1
                self._record_missing('left', right_row, 'tx_id_hex', tx_id)
        
        # Fallback join: by (block_num, tx_index_in_block) for unmatched records
        unmatched_left_blocks = set()
        unmatched_right_blocks = set()
        
        for tx_id in left_tx_idx:
            if tx_id not in right_tx_idx:
                left_row = left_tx_idx[tx_id]
                try:
                    block_num = int(left_row.get('block_num', 0))
                    tx_index = int(left_row.get('tx_index_in_block', 0))
                    unmatched_left_blocks.add((block_num, tx_index))
                except (ValueError, TypeError):
                    pass
        
        for tx_id in right_tx_idx:
            if tx_id not in left_tx_idx:
                right_row = right_tx_idx[tx_id]
                try:
                    block_num = int(right_row.get('block_num', 0))
                    tx_index = int(right_row.get('tx_index_in_block', 0))
                    unmatched_right_blocks.add((block_num, tx_index))
                except (ValueError, TypeError):
                    pass
        
        # Try fallback matches
        fallback_matches = unmatched_left_blocks & unmatched_right_blocks
        for block_key in fallback_matches:
            left_row = left_block_idx.get(block_key)
            right_row = right_block_idx.get(block_key)
            if left_row and right_row:
                print(f"Fallback match on block {block_key}")
                self._compare_rows(left_row, right_row, 'block_tx', str(block_key))
                # Adjust stats since these were counted as unmatched
                self.stats['left_only'] -= 1
                self.stats['right_only'] -= 1
    
    def _compare_rows(self, left_row: Dict, right_row: Dict, join_type: str, join_key: str) -> None:
        """Compare two matched rows field by field."""
        self.stats['matched'] += 1
        
        mismatch_info = {
            'join_type': join_type,
            'join_key': join_key,
            'left_exec_mode': left_row.get('exec_mode', ''),
            'right_exec_mode': right_row.get('exec_mode', ''),
            'block_num': left_row.get('block_num', ''),
            'tx_index': left_row.get('tx_index_in_block', ''),
            'contract_type': left_row.get('contract_type', ''),
            'mismatched_fields': {}
        }
        
        has_mismatch = False
        
        for field in self.comparison_fields:
            left_val = self.normalize_value(field, left_row.get(field, ''))
            right_val = self.normalize_value(field, right_row.get(field, ''))
            
            self.stats['total_field_comparisons'][field] += 1
            
            if not self.compare_field(field, left_val, right_val):
                self.stats['field_mismatches'][field] += 1
                mismatch_info['mismatched_fields'][field] = {
                    'left': str(left_val),
                    'right': str(right_val)
                }
                has_mismatch = True
        
        if has_mismatch:
            self.mismatches.append(mismatch_info)
    
    def _record_missing(self, missing_side: str, present_row: Dict, join_type: str, join_key: str) -> None:
        """Record a transaction that exists in only one file."""
        mismatch_info = {
            'join_type': join_type,
            'join_key': join_key,
            'missing_side': missing_side,
            'present_side': 'left' if missing_side == 'right' else 'right',
            'block_num': present_row.get('block_num', ''),
            'tx_index': present_row.get('tx_index_in_block', ''),
            'contract_type': present_row.get('contract_type', ''),
            'exec_mode': present_row.get('exec_mode', '')
        }
        self.mismatches.append(mismatch_info)
    
    def generate_reports(self) -> None:
        """Generate summary and detailed mismatch reports."""
        
        # Create output directory
        os.makedirs(self.output_dir, exist_ok=True)
        
        # Generate summary report
        summary_file = os.path.join(self.output_dir, 'comparison_summary.txt')
        self._write_summary(summary_file)
        
        # Generate detailed mismatches CSV
        mismatches_file = os.path.join(self.output_dir, 'mismatches.csv')
        self._write_mismatches_csv(mismatches_file)
        
        # Generate JSON report for programmatic analysis
        json_file = os.path.join(self.output_dir, 'comparison_results.json')
        self._write_json_report(json_file)
        
        print(f"\nReports generated in {self.output_dir}/")
        print(f"- Summary: comparison_summary.txt")
        print(f"- Mismatches: mismatches.csv")  
        print(f"- JSON data: comparison_results.json")
    
    def _write_summary(self, filename: str) -> None:
        """Write human-readable summary report."""
        with open(filename, 'w') as f:
            f.write("Execution CSV Comparison Summary\n")
            f.write("=" * 50 + "\n\n")
            
            f.write(f"Input Files:\n")
            f.write(f"  Left:  {self.left_file}\n")
            f.write(f"  Right: {self.right_file}\n\n")
            
            f.write(f"Transaction Counts:\n")
            f.write(f"  Left file:     {self.stats['left_total']:6d}\n")
            f.write(f"  Right file:    {self.stats['right_total']:6d}\n")
            f.write(f"  Matched:       {self.stats['matched']:6d}\n")
            f.write(f"  Left only:     {self.stats['left_only']:6d}\n")
            f.write(f"  Right only:    {self.stats['right_only']:6d}\n\n")
            
            # Calculate match rate
            total_possible = max(self.stats['left_total'], self.stats['right_total'])
            if total_possible > 0:
                match_rate = (self.stats['matched'] / total_possible) * 100
                f.write(f"Match Rate: {match_rate:.1f}%\n\n")
            
            f.write("Field Comparison Results:\n")
            f.write("-" * 40 + "\n")
            
            for field in self.comparison_fields:
                total_comps = self.stats['total_field_comparisons'][field]
                mismatches = self.stats['field_mismatches'][field]
                if total_comps > 0:
                    accuracy = ((total_comps - mismatches) / total_comps) * 100
                    f.write(f"  {field:20s}: {accuracy:6.1f}% ({mismatches:4d}/{total_comps:4d} mismatches)\n")
                else:
                    f.write(f"  {field:20s}: No comparisons\n")
            
            f.write(f"\nTotal Mismatched Transactions: {len(self.mismatches)}\n")
            
            if len(self.mismatches) > 0:
                f.write(f"\nTop Mismatch Categories:\n")
                # Count mismatches by contract type
                contract_type_mismatches = defaultdict(int)
                for mismatch in self.mismatches:
                    if 'contract_type' in mismatch:
                        contract_type_mismatches[mismatch['contract_type']] += 1
                
                for contract_type, count in sorted(contract_type_mismatches.items(), 
                                                 key=lambda x: x[1], reverse=True)[:10]:
                    f.write(f"  {contract_type:15s}: {count:4d}\n")
    
    def _write_mismatches_csv(self, filename: str) -> None:
        """Write detailed mismatches to CSV file."""
        if not self.mismatches:
            with open(filename, 'w') as f:
                f.write("No mismatches found.\n")
            return
        
        # Determine all possible field names for CSV header
        all_fields = set()
        for mismatch in self.mismatches:
            if 'mismatched_fields' in mismatch:
                all_fields.update(mismatch['mismatched_fields'].keys())
        
        # Create CSV header
        header = [
            'join_type', 'join_key', 'block_num', 'tx_index', 'contract_type'
        ]
        
        # Add left/right exec modes if available
        if any('left_exec_mode' in m for m in self.mismatches):
            header.extend(['left_exec_mode', 'right_exec_mode'])
        
        # Add missing side info if applicable  
        if any('missing_side' in m for m in self.mismatches):
            header.extend(['missing_side', 'present_side', 'exec_mode'])
        
        # Add field-specific columns
        for field in sorted(all_fields):
            header.extend([f'{field}_left', f'{field}_right'])
        
        with open(filename, 'w', newline='') as f:
            writer = csv.writer(f)
            writer.writerow(header)
            
            for mismatch in self.mismatches:
                row = [
                    mismatch.get('join_type', ''),
                    mismatch.get('join_key', ''),
                    mismatch.get('block_num', ''),
                    mismatch.get('tx_index', ''),
                    mismatch.get('contract_type', '')
                ]
                
                # Add exec modes if present
                if 'left_exec_mode' in mismatch:
                    row.extend([
                        mismatch.get('left_exec_mode', ''),
                        mismatch.get('right_exec_mode', '')
                    ])
                
                # Add missing side info if present
                if 'missing_side' in mismatch:
                    row.extend([
                        mismatch.get('missing_side', ''),
                        mismatch.get('present_side', ''),
                        mismatch.get('exec_mode', '')
                    ])
                
                # Add field values
                mismatched_fields = mismatch.get('mismatched_fields', {})
                for field in sorted(all_fields):
                    if field in mismatched_fields:
                        row.extend([
                            mismatched_fields[field].get('left', ''),
                            mismatched_fields[field].get('right', '')
                        ])
                    else:
                        row.extend(['', ''])
                
                writer.writerow(row)
    
    def _write_json_report(self, filename: str) -> None:
        """Write complete results as JSON for programmatic analysis."""
        report = {
            'input_files': {
                'left': self.left_file,
                'right': self.right_file
            },
            'comparison_config': {
                'fields': self.comparison_fields,
                'ignore_return_data': self.ignore_return_data
            },
            'statistics': dict(self.stats),
            'field_mismatches': dict(self.stats['field_mismatches']),
            'total_field_comparisons': dict(self.stats['total_field_comparisons']),
            'mismatches': self.mismatches
        }
        
        with open(filename, 'w') as f:
            json.dump(report, f, indent=2, default=str)
    
    def run(self) -> None:
        """Execute the complete comparison process."""
        print("Starting execution CSV comparison...")
        print(f"Left file:  {self.left_file}")
        print(f"Right file: {self.right_file}")
        print(f"Output dir: {self.output_dir}")
        print(f"Comparing fields: {', '.join(self.comparison_fields)}")
        
        # Load both CSV files
        left_tx_idx, left_block_idx = self.load_csv_with_index(self.left_file)
        right_tx_idx, right_block_idx = self.load_csv_with_index(self.right_file)
        
        # Perform join and comparison
        self.join_and_compare(left_tx_idx, left_block_idx, right_tx_idx, right_block_idx)
        
        # Generate reports
        self.generate_reports()
        
        # Print summary to console
        print(f"\nComparison complete!")
        print(f"Matched transactions: {self.stats['matched']}")
        print(f"Mismatched transactions: {len(self.mismatches)}")
        print(f"Left-only transactions: {self.stats['left_only']}")
        print(f"Right-only transactions: {self.stats['right_only']}")
        
        if self.stats['matched'] > 0:
            for field in self.comparison_fields:
                total = self.stats['total_field_comparisons'][field]
                mismatches = self.stats['field_mismatches'][field]
                if total > 0:
                    accuracy = ((total - mismatches) / total) * 100
                    print(f"{field}: {accuracy:.1f}% accuracy ({mismatches}/{total} mismatches)")


def main():
    """Main entry point."""
    parser = argparse.ArgumentParser(
        description='Compare execution consistency CSV files',
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  %(prog)s --left embedded.csv --right remote.csv --output reports/
  %(prog)s --left embedded.csv --right remote.csv --output reports/ --ignore-return-data
  %(prog)s --left embedded.csv --right remote.csv --output reports/ --fields is_success energy_used state_digest_sha256
        """
    )
    
    parser.add_argument('--left', required=True, help='Path to left CSV file (typically embedded run)')
    parser.add_argument('--right', required=True, help='Path to right CSV file (typically remote run)')
    parser.add_argument('--output', required=True, help='Output directory for reports')
    parser.add_argument('--fields', nargs='+', help='Specific fields to compare (default: all standard fields)')
    parser.add_argument('--ignore-return-data', action='store_true', 
                       help='Skip return_data_hex comparison (faster, less noise)')
    
    args = parser.parse_args()
    
    # Validate input files
    if not os.path.exists(args.left):
        print(f"Error: Left file {args.left} does not exist")
        sys.exit(1)
    if not os.path.exists(args.right):
        print(f"Error: Right file {args.right} does not exist")
        sys.exit(1)
    
    # Create and run comparator
    comparator = ExecutionComparator(
        left_file=args.left,
        right_file=args.right,
        output_dir=args.output,
        comparison_fields=args.fields,
        ignore_return_data=args.ignore_return_data
    )
    
    try:
        comparator.run()
        print("Comparison completed successfully!")
    except KeyboardInterrupt:
        print("\nComparison interrupted by user")
        sys.exit(1)
    except Exception as e:
        print(f"Error during comparison: {e}")
        sys.exit(1)


if __name__ == '__main__':
    main()