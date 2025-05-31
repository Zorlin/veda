#!/usr/bin/env python3
"""
Student score parser - reads student data from stdin and writes to files.

Input format:
- First line: number of test cases T
- For each test case:
  - Line 1: Student's first and last name
  - Line 2: Score (0-255)
"""

def parse_student_data():
    # Read number of test cases
    T = int(input())
    
    students = []
    
    # Process each test case
    for i in range(T):
        # Read student name
        name = input().strip()
        
        # Read score and validate it's in range 0-255
        score = int(input())
        if score < 0 or score > 255:
            raise ValueError(f"Score {score} is out of valid range (0-255)")
            
        students.append({
            'name': name,
            'score': score
        })
    
    return students

def write_to_file(students, filename="input.txt"):
    with open(filename, 'w') as f:
        f.write(f"Total students: {len(students)}\n")
        f.write("-" * 40 + "\n")
        
        for i, student in enumerate(students, 1):
            f.write(f"Student {i}:\n")
            f.write(f"  Name: {student['name']}\n")
            f.write(f"  Score: {student['score']}/255\n")
            f.write(f"  Percentage: {(student['score']/255*100):.1f}%\n")
            f.write("-" * 40 + "\n")

def main():
    try:
        # Parse student data from stdin
        students = parse_student_data()
        
        # Write to input.txt
        write_to_file(students)
        
        print(f"Successfully processed {len(students)} students.")
        print("Data written to input.txt")
        
    except ValueError as e:
        print(f"Error: {e}")
    except Exception as e:
        print(f"Unexpected error: {e}")

if __name__ == "__main__":
    main()