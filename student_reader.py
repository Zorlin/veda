#!/usr/bin/env python3
"""
Student Score Reader
Reads student information from stdin and writes to input.txt

Input format:
- First line: number of test cases T
- For each test case:
  - Line 1: Student's first and last name
  - Line 2: Score (0-255)
"""

def main():
    # Read number of test cases
    T = int(input())
    
    # Open output file
    with open("input.txt", "w") as output_file:
        # Write header
        output_file.write(f"Total students: {T}\n")
        output_file.write("-" * 40 + "\n")
        
        # Process each test case
        for i in range(T):
            # Read student name
            name = input().strip()
            
            # Read score
            score = int(input())
            
            # Validate score (0-255 range)
            if score < 0 or score > 255:
                print(f"Warning: Score {score} for {name} is out of valid range (0-255)")
            
            # Write to output file
            output_file.write(f"Student {i+1}:\n")
            output_file.write(f"  Name: {name}\n")
            output_file.write(f"  Score: {score}/255\n")
            output_file.write(f"  Percentage: {(score/255)*100:.2f}%\n")
            output_file.write("-" * 40 + "\n")
    
    print("Data written to input.txt successfully!")

if __name__ == "__main__":
    main()